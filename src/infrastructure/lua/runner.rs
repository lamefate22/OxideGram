//! Lua bot runner managing script evaluation, update streaming, and event dispatching.

use crate::application::automation::BotConsole;
use crate::domain::automation::{ChatType, FilterMatchResult, MessageContext, MessageMarkup};
use crate::domain::types::{ChatId, SenderId};
use crate::errors::{OxideError, ScriptError};
use crate::infrastructure::lua::api::{
    MessageOptions, RegisteredHandler, TimerHub, apply_optional_delay, build_input_message,
    invoke_send_message, invoke_with_flood_wait, parse_message_options, register_ox_table,
    resolve_peer_ref,
};
use crate::infrastructure::lua::buttons::{
    ReplyMarkupAction, markup_to_lua_table, parse_reply_markup_action,
};
use crate::infrastructure::{IO_TIMEOUT, NETWORK_TIMEOUT};
use grammers_client::Client;
use grammers_client::message::InputMessage;
use grammers_client::update::Update;
use grammers_mtsender::UpdatesConfiguration;
use grammers_session::types::{PeerKind, PeerRef};
use grammers_session::updates::UpdatesLike;
use mlua::{Function, Lua, RegistryKey, Value};
use notify::{EventKind, RecursiveMode, Watcher};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::sync::{Mutex, RwLock, watch};
use tracing::{debug, error, info, warn};

/// Runner responsible for binding Telegram methods to Lua, loading bot scripts, and handling updates.
pub struct LuaBotRunner {
    pub client: Client,
    pub updates: Option<mpsc::Receiver<UpdatesLike>>,
    pub lua: Lua,
    pub message_handlers: Arc<Mutex<Vec<RegisteredHandler>>>,
    pub chat_markups: Arc<RwLock<HashMap<i64, MessageMarkup>>>,
    peer_refs: Arc<RwLock<HashMap<i64, PeerRef>>>,
    console: Arc<dyn BotConsole>,
    stop_tx: watch::Sender<bool>,
    stop_rx: watch::Receiver<bool>,
    timer_hub: Arc<TimerHub>,
    timer_rx: mpsc::Receiver<Arc<RegistryKey>>,
    script_path: Option<PathBuf>,
}

impl LuaBotRunner {
    /// Creates a new `LuaBotRunner` with an authorized Telegram client and updates channel.
    pub fn new(
        client: Client,
        updates: mpsc::Receiver<UpdatesLike>,
        console: Arc<dyn BotConsole>,
    ) -> Result<Self, OxideError> {
        let lua = Lua::new();
        let message_handlers = Arc::new(Mutex::new(Vec::new()));
        let peer_refs = Arc::new(RwLock::new(HashMap::new()));
        let chat_markups = Arc::new(RwLock::new(HashMap::new()));
        let (stop_tx, stop_rx) = watch::channel(false);
        let (timer_tx, timer_rx) = mpsc::channel(100);
        let timer_hub = Arc::new(TimerHub::new(timer_tx));

        Ok(Self {
            client,
            updates: Some(updates),
            lua,
            message_handlers,
            chat_markups,
            peer_refs,
            console,
            stop_tx,
            stop_rx,
            timer_hub,
            timer_rx,
            script_path: None,
        })
    }

    /// Loads and evaluates a `.lua` bot script file, exposing global `ox` table helper functions.
    pub async fn load_script<P: AsRef<Path>>(&mut self, script_path: P) -> Result<(), OxideError> {
        let path = script_path.as_ref().to_path_buf();
        self.script_path = Some(path.clone());
        let content = tokio::time::timeout(IO_TIMEOUT, tokio::fs::read_to_string(&path))
            .await
            .map_err(|_| ScriptError::Timeout("Read script file timed out".to_string()))?
            .map_err(|source| ScriptError::ScriptIo {
                path: path.clone(),
                source,
            })?;

        let script_stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("default");
        let storage =
            crate::infrastructure::lua::storage::BotStorage::open("data/storage", script_stem)
                .await;

        register_ox_table(
            &self.lua,
            self.client.clone(),
            Arc::clone(&self.message_handlers),
            Arc::clone(&self.peer_refs),
            Arc::clone(&self.console),
            self.stop_tx.clone(),
            Arc::clone(&self.timer_hub),
            storage,
        )?;

        self.lua
            .load(&content)
            .exec()
            .map_err(ScriptError::LuaError)?;

        let handler_count = self.message_handlers.lock().await.len();
        info!(path = %path.display(), handler_count, "Lua script loaded");
        Ok(())
    }

    /// Reloads the bot script without terminating the Telegram session or update connection.
    pub async fn reload_script(&mut self) -> Result<(), OxideError> {
        let Some(ref path) = self.script_path else {
            return Err(
                ScriptError::Runtime("No script path configured for hot reload".into()).into(),
            );
        };

        let content = tokio::time::timeout(IO_TIMEOUT, tokio::fs::read_to_string(path))
            .await
            .map_err(|_| ScriptError::Timeout("Read script file timed out".to_string()))?
            .map_err(|source| ScriptError::ScriptIo {
                path: path.clone(),
                source,
            })?;

        // Cancel existing timers and flush channel
        self.timer_hub.cancel_all().await;
        while self.timer_rx.try_recv().is_ok() {}
        self.message_handlers.lock().await.clear();

        // Fresh Lua state
        self.lua = Lua::new();

        let script_stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("default");
        let storage =
            crate::infrastructure::lua::storage::BotStorage::open("data/storage", script_stem)
                .await;

        register_ox_table(
            &self.lua,
            self.client.clone(),
            Arc::clone(&self.message_handlers),
            Arc::clone(&self.peer_refs),
            Arc::clone(&self.console),
            self.stop_tx.clone(),
            Arc::clone(&self.timer_hub),
            storage,
        )?;

        self.lua
            .load(&content)
            .exec()
            .map_err(ScriptError::LuaError)?;

        let handler_count = self.message_handlers.lock().await.len();
        info!(path = %path.display(), handler_count, "Lua script hot-reloaded successfully");
        Ok(())
    }

    /// Starts listening for incoming MTProto updates and invokes registered Lua handlers matching filters.
    ///
    pub async fn run_event_loop(&mut self) -> Result<(), OxideError> {
        let handler_count = self.message_handlers.lock().await.len();
        info!(handler_count, "Bot event loop started");
        let updates = self
            .updates
            .take()
            .ok_or_else(|| ScriptError::UpdateStream("Updates receiver already consumed".into()))?;
        let mut stop_rx = self.stop_rx.clone();
        let mut stream = tokio::time::timeout(
            NETWORK_TIMEOUT,
            self.client
                .stream_updates(updates, UpdatesConfiguration::default()),
        )
        .await
        .map_err(|_| ScriptError::Timeout("Stream updates timed out".into()))?
        .map_err(|e| ScriptError::UpdateStream(e.to_string()))?;

        let (update_tx, mut update_rx) = mpsc::channel::<Update>(100);
        let mut reader_stop_rx = stop_rx.clone();

        // Background reader task: continuously polls Telegram updates without being blocked by Lua handlers
        let reader_handle = tokio::spawn(async move {
            loop {
                tokio::select! {
                    biased;
                    changed = reader_stop_rx.changed() => {
                        if changed.is_ok() && *reader_stop_rx.borrow() {
                            break;
                        }
                    }
                    next = stream.next() => {
                        match next {
                            Ok(update) => {
                                if update_tx.send(update).await.is_err() {
                                    break;
                                }
                            }
                            Err(error) => {
                                warn!(error = %error, "Telegram update stream error, retrying in 2 seconds...");
                                tokio::select! {
                                    _ = reader_stop_rx.changed() => break,
                                    _ = tokio::time::sleep(std::time::Duration::from_secs(2)) => continue,
                                }
                            }
                        }
                    }
                }
            }

            // Synchronize updates state with session before shutdown
            if let Err(error) =
                tokio::time::timeout(NETWORK_TIMEOUT, stream.sync_update_state()).await
            {
                warn!(error = %error, "Failed to synchronize update state with session");
            }
        });

        // Setup filesystem watcher for hot reload if a script file was specified
        let (fs_tx, mut fs_rx) = mpsc::channel::<notify::Result<notify::Event>>(16);
        let mut _watcher = None;
        if let Some(ref path) = self.script_path {
            let tx = fs_tx.clone();
            match notify::recommended_watcher(move |res| {
                let _ = tx.blocking_send(res);
            }) {
                Ok(mut w) => {
                    let watch_target = path.parent().unwrap_or(path);
                    if let Err(e) = w.watch(watch_target, RecursiveMode::NonRecursive) {
                        warn!(error = %e, "Failed to watch directory for script changes");
                    } else {
                        info!(path = %watch_target.display(), "Hot-reload watcher activated");
                        _watcher = Some(w);
                    }
                }
                Err(e) => {
                    warn!(error = %e, "Failed to initialize filesystem watcher");
                }
            }
        }

        let ctrl_c = tokio::signal::ctrl_c();
        tokio::pin!(ctrl_c);

        loop {
            if *stop_rx.borrow() {
                info!("Bot event loop stopped by Lua script");
                break;
            }

            tokio::select! {
                biased;
                changed = stop_rx.changed() => {
                    if changed.is_ok() && *stop_rx.borrow() {
                        info!("Bot event loop stopped by Lua script");
                        break;
                    }
                    continue;
                }
                signal = &mut ctrl_c => {
                    match signal {
                        Ok(()) => info!("Bot event loop stopped by Ctrl+C"),
                        Err(error) => warn!(error = %error, "Failed to listen for Ctrl+C"),
                    }
                    self.stop_tx.send_replace(true);
                    break;
                }
                Some(timer_key) = self.timer_rx.recv() => {
                    if let Ok(func) = self.lua.registry_value::<Function>(&timer_key)
                        && let Err(err) = func.call_async::<()>(()).await
                    {
                        error!(error = %err, "Lua timer callback execution failed");
                    }
                }
                Some(fs_res) = fs_rx.recv() => {
                    if let Ok(event) = fs_res
                        && matches!(event.kind, EventKind::Modify(_) | EventKind::Create(_))
                    {
                        let target_name = self.script_path.as_ref().and_then(|p| p.file_name());
                        let matches_target = event.paths.iter().any(|p| p.file_name() == target_name);
                        if matches_target {
                            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                            while fs_rx.try_recv().is_ok() {}
                            info!("Script change detected. Initiating hot-reload...");
                            if let Err(err) = self.reload_script().await {
                                error!(error = %err, "Failed to hot-reload bot script");
                            }
                        }
                    }
                }
                update = update_rx.recv() => {
                    let Some(update) = update else {
                        warn!("Update receiver channel closed");
                        break;
                    };

                    if let Update::NewMessage(message) = update {
                        self.dispatch_message(message, &stop_rx).await?;
                    }
                }
            }
        }

        self.timer_hub.cancel_all().await;
        self.stop_tx.send_replace(true);
        let _ = reader_handle.await;
        Ok(())
    }

    /// Dispatches a single incoming Telegram message to registered Lua handlers matching filters.
    async fn dispatch_message(
        &self,
        message: grammers_client::update::Message,
        stop_rx: &watch::Receiver<bool>,
    ) -> Result<(), OxideError> {
        let handlers = self.message_handlers.lock().await;
        if handlers.is_empty() {
            return Ok(());
        }

        let text = message.text().to_string();
        let Some(chat_id) = message.peer_id().bare_id() else {
            warn!("Skipping message with unresolved chat ID");
            return Ok(());
        };

        let peer_ref_res = tokio::time::timeout(NETWORK_TIMEOUT, message.peer_ref()).await;
        let peer_ref = match peer_ref_res {
            Ok(Ok(Some(peer_ref))) => peer_ref,
            Ok(Ok(None)) => match resolve_peer_ref(&self.client, &self.peer_refs, chat_id).await {
                Ok(peer_ref) => peer_ref,
                Err(_) => {
                    warn!(chat_id, "Skipping message with unresolved Telegram peer");
                    return Ok(());
                }
            },
            Ok(Err(error)) => {
                warn!(chat_id, error = %error, "Failed to resolve Telegram peer");
                return Ok(());
            }
            Err(_) => {
                warn!(chat_id, "Resolving Telegram peer timed out");
                return Ok(());
            }
        };

        self.peer_refs.write().await.insert(chat_id, peer_ref);
        let sender_id = message.sender_id().and_then(|id| id.bare_id()).unwrap_or(0);
        let outgoing = message.outgoing();
        let incoming = !outgoing;
        let chat_type = match message.peer_id().kind() {
            PeerKind::User => ChatType::Private,
            PeerKind::Chat => ChatType::Group,
            PeerKind::Channel => ChatType::Channel,
        };

        debug!(
            chat_id,
            sender_id,
            incoming,
            text_len = text.len(),
            "Telegram message received"
        );

        let context = MessageContext {
            text: &text,
            chat_id: ChatId::new(chat_id),
            sender_id: SenderId::new(sender_id),
            incoming,
            chat_type,
        };

        let markup_action = parse_reply_markup_action(message.reply_markup().as_ref());
        let effective_markup = match markup_action {
            ReplyMarkupAction::Inline(inline_markup) => {
                let cached_opt = self.chat_markups.read().await.get(&chat_id).cloned();
                if let Some(cached) = cached_opt {
                    inline_markup.merge(&cached)
                } else {
                    inline_markup
                }
            }
            ReplyMarkupAction::ReplyKeyboard(reply_markup) => {
                self.chat_markups
                    .write()
                    .await
                    .insert(chat_id, reply_markup.clone());
                reply_markup
            }
            ReplyMarkupAction::HideKeyboard => {
                self.chat_markups.write().await.remove(&chat_id);
                MessageMarkup::default()
            }
            ReplyMarkupAction::None => self
                .chat_markups
                .read()
                .await
                .get(&chat_id)
                .cloned()
                .unwrap_or_default(),
        };

        for item in handlers.iter() {
            let match_result = if let Some(ref filter) = item.filter {
                match filter.match_context(&context) {
                    Some(res) => res,
                    None => {
                        debug!(chat_id, sender_id, "Lua message filter did not match");
                        continue;
                    }
                }
            } else {
                FilterMatchResult::default()
            };

            let event_table = self.create_event_table(
                &message,
                &text,
                chat_id,
                sender_id,
                incoming,
                outgoing,
                chat_type,
                peer_ref,
                &effective_markup,
                &match_result,
            )?;

            let clean = text.replace(['\r', '\n'], " ");
            let snippet = if clean.chars().count() > 40 {
                let s: String = clean.chars().take(40).collect();
                format!("{s}...")
            } else {
                clean
            };
            debug!(target: "oxidegram", "[MSG] [{chat_id}] \"{snippet}\"");

            debug!(chat_id, sender_id, "Running Lua message handler");
            if let Ok(func) = self.lua.registry_value::<Function>(&item.callback_key)
                && let Err(err) = func.call_async::<()>(event_table).await
            {
                error!(chat_id, sender_id, error = %err, "Lua message handler failed");
            }
            if *stop_rx.borrow() {
                break;
            }
        }

        Ok(())
    }

    /// Checks if a Lua value is the event table instance.
    fn is_event_table(val: &Value) -> bool {
        if let Value::Table(t) = val {
            t.contains_key("incoming").unwrap_or(false)
                && t.contains_key("chat_id").unwrap_or(false)
        } else {
            false
        }
    }

    /// Helper to shift Lua arguments when a method is called with colon syntax `event:method(...)`
    /// where `arg1` is the `event` table (`self`).
    fn shift_event_args(
        a1: Value,
        a2: Option<Value>,
        a3: Option<Value>,
    ) -> (Value, Option<Value>, Option<Value>) {
        if Self::is_event_table(&a1) {
            (a2.unwrap_or(Value::Nil), a3, None)
        } else {
            (a1, a2, a3)
        }
    }

    /// Helper to convert optional Lua number/integer to f64.
    fn value_to_f64(val: Option<Value>) -> Option<f64> {
        match val {
            Some(Value::Number(n)) => Some(n),
            Some(Value::Integer(i)) => Some(i as f64),
            _ => None,
        }
    }

    /// Constructs the Lua event table and binds helper methods.
    #[allow(clippy::too_many_arguments)]
    fn create_event_table(
        &self,
        message: &grammers_client::update::Message,
        text: &str,
        chat_id: i64,
        sender_id: i64,
        incoming: bool,
        outgoing: bool,
        chat_type: ChatType,
        peer_ref: PeerRef,
        markup: &MessageMarkup,
        match_result: &FilterMatchResult,
    ) -> Result<mlua::Table, OxideError> {
        let event_table = self.lua.create_table().map_err(ScriptError::LuaError)?;

        let message_id = message.id();
        event_table
            .set("id", message_id)
            .map_err(ScriptError::LuaError)?;
        event_table
            .set("message_id", message_id)
            .map_err(ScriptError::LuaError)?;
        event_table
            .set("text", text)
            .map_err(ScriptError::LuaError)?;
        event_table
            .set("chat_id", chat_id)
            .map_err(ScriptError::LuaError)?;
        event_table
            .set("sender_id", sender_id)
            .map_err(ScriptError::LuaError)?;
        event_table
            .set("incoming", incoming)
            .map_err(ScriptError::LuaError)?;
        event_table
            .set("outgoing", outgoing)
            .map_err(ScriptError::LuaError)?;
        event_table
            .set("is_private", chat_type == ChatType::Private)
            .map_err(ScriptError::LuaError)?;
        event_table
            .set("is_group", chat_type == ChatType::Group)
            .map_err(ScriptError::LuaError)?;
        event_table
            .set("is_channel", chat_type == ChatType::Channel)
            .map_err(ScriptError::LuaError)?;

        // Regex matches and named captures
        let matches_table = self.lua.create_table().map_err(ScriptError::LuaError)?;
        for (i, m) in match_result.matches.iter().enumerate() {
            matches_table
                .raw_set(i + 1, m.clone())
                .map_err(ScriptError::LuaError)?;
        }
        for (k, v) in &match_result.captures {
            matches_table
                .raw_set(k.as_str(), v.clone())
                .map_err(ScriptError::LuaError)?;
        }
        event_table
            .set("matches", matches_table.clone())
            .map_err(ScriptError::LuaError)?;
        event_table
            .set("captures", matches_table)
            .map_err(ScriptError::LuaError)?;

        // Parsed inline/reply buttons
        let buttons_table =
            markup_to_lua_table(&self.lua, markup).map_err(ScriptError::LuaError)?;
        event_table
            .set("buttons", buttons_table)
            .map_err(ScriptError::LuaError)?;

        // event.reply(reply_text, [options_or_delay]) OR event:reply { text = ..., delay = ... }
        let client_reply = self.client.clone();
        let reply_fn = self
            .lua
            .create_async_function(
                move |_, (a1, a2, a3): (Value, Option<Value>, Option<Value>)| {
                    let client = client_reply.clone();
                    async move {
                        let (arg1, opt_val, _) = Self::shift_event_args(a1, a2, a3);
                        let (reply_text, opts) = if let Value::Table(ref t) = arg1 {
                            if t.contains_key("text")? || t.contains_key("message")? {
                                let text: String = t
                                    .get("text")
                                    .or_else(|_| t.get("message"))
                                    .map_err(|e| mlua::Error::RuntimeError(e.to_string()))?;
                                let delay: Option<f64> = t.get("delay").ok();
                                let parse_mode: Option<String> = t.get("parse_mode").ok();
                                (text, MessageOptions { delay, parse_mode })
                            } else {
                                return Err(mlua::Error::RuntimeError(
                                    "reply named table requires 'text'".into(),
                                ));
                            }
                        } else {
                            let text = match arg1 {
                                Value::String(s) => s.to_str()?.to_string(),
                                Value::Integer(i) => i.to_string(),
                                Value::Number(n) => n.to_string(),
                                _ => {
                                    return Err(mlua::Error::RuntimeError(
                                        "reply expects text message or table".into(),
                                    ));
                                }
                            };
                            let opts = parse_message_options(opt_val)?;
                            (text, opts)
                        };

                        apply_optional_delay(opts.delay).await;

                        let reply_message =
                            build_input_message(&reply_text, opts.parse_mode.as_deref())
                                .reply_to(Some(message_id));

                        invoke_send_message(&client, peer_ref, reply_message).await?;
                        let clean = reply_text.replace(['\r', '\n'], " ");
                        let snippet = if clean.chars().count() > 40 {
                            let s: String = clean.chars().take(40).collect();
                            format!("{s}...")
                        } else {
                            clean
                        };
                        info!(target: "oxidegram", "[REPLY] [{chat_id}] \"{snippet}\"");
                        Ok(())
                    }
                },
            )
            .map_err(ScriptError::LuaError)?;
        event_table
            .set("reply", reply_fn)
            .map_err(ScriptError::LuaError)?;

        // event.edit(new_text, [options_or_delay]) OR event:edit { text = ..., delay = ... }
        let client_edit = self.client.clone();
        let edit_fn = self
            .lua
            .create_async_function(
                move |_, (a1, a2, a3): (Value, Option<Value>, Option<Value>)| {
                    let client = client_edit.clone();
                    async move {
                        let (arg1, opt_val, _) = Self::shift_event_args(a1, a2, a3);
                        let (new_text, opts) = if let Value::Table(ref t) = arg1 {
                            if t.contains_key("text")? || t.contains_key("message")? {
                                let text: String = t
                                    .get("text")
                                    .or_else(|_| t.get("message"))
                                    .map_err(|e| mlua::Error::RuntimeError(e.to_string()))?;
                                let delay: Option<f64> = t.get("delay").ok();
                                let parse_mode: Option<String> = t.get("parse_mode").ok();
                                (text, MessageOptions { delay, parse_mode })
                            } else {
                                return Err(mlua::Error::RuntimeError(
                                    "edit named table requires 'text'".into(),
                                ));
                            }
                        } else {
                            let text = match arg1 {
                                Value::String(s) => s.to_str()?.to_string(),
                                Value::Integer(i) => i.to_string(),
                                Value::Number(n) => n.to_string(),
                                _ => {
                                    return Err(mlua::Error::RuntimeError(
                                        "edit expects new text as first argument".into(),
                                    ));
                                }
                            };
                            let opts = parse_message_options(opt_val)?;
                            (text, opts)
                        };

                        apply_optional_delay(opts.delay).await;
                        let input_msg = build_input_message(&new_text, opts.parse_mode.as_deref());

                        tokio::time::timeout(
                            NETWORK_TIMEOUT,
                            client.edit_message(peer_ref, message_id, input_msg),
                        )
                        .await
                        .map_err(|_| mlua::Error::RuntimeError("Edit message timed out".into()))?
                        .map_err(|e| {
                            mlua::Error::RuntimeError(format!("Edit message error: {e}"))
                        })?;

                        Ok(())
                    }
                },
            )
            .map_err(ScriptError::LuaError)?;
        event_table
            .set("edit", edit_fn)
            .map_err(ScriptError::LuaError)?;

        // event.delete([delay]) OR event:delete { delay = ... }
        let client_del = self.client.clone();
        let delete_fn = self
            .lua
            .create_async_function(move |_, (a1, a2): (Option<Value>, Option<Value>)| {
                let client = client_del.clone();
                async move {
                    let delay = if a1.as_ref().map(Self::is_event_table).unwrap_or(false) {
                        if let Some(Value::Table(ref t)) = a2 {
                            t.get::<Option<f64>>("delay").unwrap_or(None)
                        } else {
                            Self::value_to_f64(a2)
                        }
                    } else if let Some(Value::Table(ref t)) = a1 {
                        t.get::<Option<f64>>("delay").unwrap_or(None)
                    } else {
                        Self::value_to_f64(a1)
                    };

                    apply_optional_delay(delay).await;
                    tokio::time::timeout(
                        NETWORK_TIMEOUT,
                        client.delete_messages(peer_ref, &[message_id]),
                    )
                    .await
                    .map_err(|_| mlua::Error::RuntimeError("Delete message timed out".into()))?
                    .map_err(|e| mlua::Error::RuntimeError(format!("Delete message error: {e}")))?;
                    Ok(())
                }
            })
            .map_err(ScriptError::LuaError)?;
        event_table
            .set("delete", delete_fn)
            .map_err(ScriptError::LuaError)?;

        // event.react(emoji, [delay]) OR event:react { emoji = ..., delay = ... }
        let client_react = self.client.clone();
        let react_fn = self
            .lua
            .create_async_function(
                move |_, (a1, a2, a3): (Value, Option<Value>, Option<Value>)| {
                    let client = client_react.clone();
                    async move {
                        let (arg1, delay_val, _) = Self::shift_event_args(a1, a2, a3);
                        let (emoji, delay) = if let Value::Table(ref t) = arg1 {
                            let e: String = t
                                .get("reaction")
                                .or_else(|_| t.get("emoji"))
                                .map_err(|e| mlua::Error::RuntimeError(e.to_string()))?;
                            let d: Option<f64> = t.get("delay").ok();
                            (e, d)
                        } else {
                            let e = match arg1 {
                                Value::String(s) => s.to_str()?.to_string(),
                                _ => {
                                    return Err(mlua::Error::RuntimeError(
                                        "react expects emoji string or table".into(),
                                    ));
                                }
                            };
                            (e, Self::value_to_f64(delay_val))
                        };

                        apply_optional_delay(delay).await;
                        tokio::time::timeout(
                            NETWORK_TIMEOUT,
                            client.send_reactions(peer_ref, message_id, emoji.as_str()),
                        )
                        .await
                        .map_err(|_| mlua::Error::RuntimeError("Send reaction timed out".into()))?
                        .map_err(|e| {
                            mlua::Error::RuntimeError(format!("Send reaction error: {e}"))
                        })?;
                        Ok(())
                    }
                },
            )
            .map_err(ScriptError::LuaError)?;
        event_table
            .set("react", react_fn)
            .map_err(ScriptError::LuaError)?;

        // event.pin([delay]) OR event:pin { delay = ... }
        let client_pin = self.client.clone();
        let pin_fn = self
            .lua
            .create_async_function(move |_, (a1, a2): (Option<Value>, Option<Value>)| {
                let client = client_pin.clone();
                async move {
                    let delay = if a1.as_ref().map(Self::is_event_table).unwrap_or(false) {
                        if let Some(Value::Table(ref t)) = a2 {
                            t.get::<Option<f64>>("delay").unwrap_or(None)
                        } else {
                            Self::value_to_f64(a2)
                        }
                    } else if let Some(Value::Table(ref t)) = a1 {
                        t.get::<Option<f64>>("delay").unwrap_or(None)
                    } else {
                        Self::value_to_f64(a1)
                    };

                    apply_optional_delay(delay).await;
                    tokio::time::timeout(NETWORK_TIMEOUT, client.pin_message(peer_ref, message_id))
                        .await
                        .map_err(|_| mlua::Error::RuntimeError("Pin message timed out".into()))?
                        .map_err(|e| {
                            mlua::Error::RuntimeError(format!("Pin message error: {e}"))
                        })?;
                    Ok(())
                }
            })
            .map_err(ScriptError::LuaError)?;
        event_table
            .set("pin", pin_fn)
            .map_err(ScriptError::LuaError)?;

        // event.click(query_or_index, [delay]) / event.click_button
        let client_click = self.client.clone();
        let markup_click = markup.clone();
        let click_fn = self
            .lua
            .create_async_function(
                move |_, (a1, a2, a3): (Value, Option<Value>, Option<Value>)| {
                    let client = client_click.clone();
                    let markup = markup_click.clone();
                    async move {
                        let (arg1, delay_val, _) = Self::shift_event_args(a1, a2, a3);
                        let (query_val, delay) = if let Value::Table(ref t) = arg1 {
                            let q: Value = t
                                .get("query")
                                .or_else(|_| t.get("text"))
                                .or_else(|_| t.get("index"))
                                .map_err(|e| mlua::Error::RuntimeError(e.to_string()))?;
                            let d: Option<f64> = t.get("delay").ok();
                            (q, d)
                        } else {
                            (arg1, Self::value_to_f64(delay_val))
                        };

                        apply_optional_delay(delay).await;
                        let btn = match &query_val {
                            Value::Integer(i) if *i >= 1 => {
                                markup.flat_button_at((*i - 1) as usize)
                            }
                            Value::Number(n) if *n >= 1.0 => {
                                markup.flat_button_at((*n as usize) - 1)
                            }
                            Value::String(s) => {
                                let query_str = s
                                    .to_str()
                                    .map_err(|e| mlua::Error::RuntimeError(e.to_string()))?;
                                markup.find_button(&query_str).map(|(_, _, b)| b)
                            }
                            _ => {
                                return Err(mlua::Error::RuntimeError(
                                    "click expects button text or 1-based index".into(),
                                ));
                            }
                        };

                        if let Some(button) = btn {
                            info!(target: "oxidegram", "[ACT] Clicked button: \"{}\"", button.text);
                            match &button.kind {
                                crate::domain::automation::ButtonKind::Callback(data) => {
                                    use grammers_tl_types as tl;
                                    let input_peer = tl::enums::InputPeer::from(&peer_ref);
                                    let req = tl::functions::messages::GetBotCallbackAnswer {
                                        peer: input_peer,
                                        msg_id: message_id,
                                        data: Some(data.clone()),
                                        game: false,
                                        password: None,
                                    };
                                    let res = invoke_with_flood_wait(&client, &req).await?;
                                    match res {
                                        tl::enums::messages::BotCallbackAnswer::Answer(ans) => {
                                            Ok(ans.message.unwrap_or_default())
                                        }
                                    }
                                }
                                crate::domain::automation::ButtonKind::Url(url) => Ok(url.clone()),
                                crate::domain::automation::ButtonKind::Text => {
                                    let input_msg = InputMessage::new().text(button.text.clone());
                                    invoke_send_message(&client, peer_ref, input_msg).await?;
                                    Ok(button.text.clone())
                                }
                                _ => Ok(String::new()),
                            }
                        } else if let Value::String(s) = query_val {
                            // In Telegram, clicking a regular reply button simply sends its text as a message.
                            // If the button wasn't explicitly found in the parsed keyboard (e.g. sent in earlier message),
                            // send the requested text directly as a resilient fallback.
                            let text = s
                                .to_str()
                                .map_err(|e| mlua::Error::RuntimeError(e.to_string()))?
                                .to_string();
                            info!(target: "oxidegram", "[ACT] Pressed reply button: \"{}\"", text);
                            let input_msg = InputMessage::new().text(text.clone());
                            invoke_send_message(&client, peer_ref, input_msg).await?;
                            Ok(text)
                        } else {
                            Err(mlua::Error::RuntimeError(
                                "Button not found on message".into(),
                            ))
                        }
                    }
                },
            )
            .map_err(ScriptError::LuaError)?;
        event_table
            .set("click", click_fn)
            .map_err(ScriptError::LuaError)?;

        Ok(event_table)
    }
}
