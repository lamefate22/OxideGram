//! Lua bot runtime environment built on top of `mlua` and Tokio async tasks.
//!
//! Exposes helper functions (`ox.send_message`, `ox.reply`, `ox.on_message`, `ox.stop`) to Lua bot
//! scripts, supports Telethon-like filters for peers, direction, chat type, text, commands, and
//! patterns, listens for MTProto updates, and dispatches matching messages to registered callbacks.

use crate::application::automation::BotConsole;
use crate::domain::automation::{ChatType, MessageContext, MessageFilter};
use crate::errors::{OxideError, ScriptError};
use grammers_client::Client;
use grammers_client::client::UpdatesConfiguration;
use grammers_client::message::InputMessage;
use grammers_client::update::Update;
use grammers_session::types::{PeerKind, PeerRef};
use grammers_session::updates::UpdatesLike;
use mlua::{Function, Lua, RegistryKey, Table, Value};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::sync::{Mutex, RwLock, watch};
use tracing::{debug, error, info, warn};

/// Converts Lua filter values into the domain filter model.
fn parse_message_filter(table: &Table) -> Result<MessageFilter, mlua::Error> {
    let mut filter = MessageFilter {
        chats: parse_integer_list(table, "chats")?,
        senders: parse_integer_list(table, "senders")?,
        incoming: parse_boolean(table, "incoming")?,
        outgoing: parse_boolean(table, "outgoing")?,
        private: parse_boolean(table, "private")?,
        group: parse_boolean(table, "group")?,
        channel: parse_boolean(table, "channel")?,
        has_text: parse_boolean(table, "has_text")?,
        commands: parse_string_list(table, "commands")?.map(|commands| {
            commands
                .into_iter()
                .map(|command| command.trim_start_matches('/').to_lowercase())
                .collect()
        }),
        ..MessageFilter::default()
    };

    if let Some(pattern) = parse_string(table, "pattern")? {
        filter.pattern = Some(regex::Regex::new(&pattern).map_err(|error| {
            mlua::Error::RuntimeError(format!("invalid 'pattern' regex '{pattern}': {error}"))
        })?);
    }
    Ok(filter)
}

fn parse_boolean(table: &Table, key: &str) -> Result<Option<bool>, mlua::Error> {
    match table.get::<Value>(key)? {
        Value::Nil => Ok(None),
        Value::Boolean(value) => Ok(Some(value)),
        value => Err(invalid_filter_type(key, "boolean", &value)),
    }
}

fn parse_string(table: &Table, key: &str) -> Result<Option<String>, mlua::Error> {
    match table.get::<Value>(key)? {
        Value::Nil => Ok(None),
        Value::String(value) => Ok(Some(value.to_str()?.to_string())),
        value => Err(invalid_filter_type(key, "string", &value)),
    }
}

fn parse_integer_list(table: &Table, key: &str) -> Result<Option<Vec<i64>>, mlua::Error> {
    match table.get::<Value>(key)? {
        Value::Nil => Ok(None),
        Value::Integer(value) => Ok(Some(vec![value])),
        Value::Table(values) => {
            let values = values
                .sequence_values::<i64>()
                .collect::<Result<Vec<_>, _>>()?;
            if values.is_empty() {
                return Err(mlua::Error::RuntimeError(format!(
                    "filter '{key}' cannot be empty"
                )));
            }
            Ok(Some(values))
        }
        value => Err(invalid_filter_type(key, "integer or integer array", &value)),
    }
}

fn parse_string_list(table: &Table, key: &str) -> Result<Option<Vec<String>>, mlua::Error> {
    match table.get::<Value>(key)? {
        Value::Nil => Ok(None),
        Value::String(value) => Ok(Some(vec![value.to_str()?.to_string()])),
        Value::Table(values) => {
            let values = values
                .sequence_values::<String>()
                .collect::<Result<Vec<_>, _>>()?;
            if values.is_empty() {
                return Err(mlua::Error::RuntimeError(format!(
                    "filter '{key}' cannot be empty"
                )));
            }
            Ok(Some(values))
        }
        value => Err(invalid_filter_type(key, "string or string array", &value)),
    }
}

fn invalid_filter_type(key: &str, expected: &str, value: &Value) -> mlua::Error {
    mlua::Error::RuntimeError(format!(
        "filter '{key}' must be {expected}, got {}",
        value.type_name()
    ))
}

fn create_stop_function(lua: &Lua, stop_tx: watch::Sender<bool>) -> mlua::Result<Function> {
    lua.create_function(move |_, ()| {
        stop_tx.send_replace(true);
        Ok(())
    })
}

/// Registered message handler entry containing Lua callback key and optional filter.
pub struct RegisteredHandler {
    pub callback_key: RegistryKey,
    pub filter: Option<MessageFilter>,
}

/// Runner responsible for binding Telegram methods to Lua, loading bot scripts, and handling updates.
pub struct LuaBotRunner {
    pub client: Client,
    pub updates: mpsc::UnboundedReceiver<UpdatesLike>,
    pub lua: Lua,
    pub message_handlers: Arc<Mutex<Vec<RegisteredHandler>>>,
    peer_refs: Arc<RwLock<HashMap<i64, PeerRef>>>,
    console: Arc<dyn BotConsole>,
    stop_tx: watch::Sender<bool>,
    stop_rx: watch::Receiver<bool>,
}

impl LuaBotRunner {
    /// Creates a new `LuaBotRunner` with an authorized Telegram client and updates channel.
    ///
    pub fn new(
        client: Client,
        updates: mpsc::UnboundedReceiver<UpdatesLike>,
        console: Arc<dyn BotConsole>,
    ) -> Result<Self, OxideError> {
        let lua = Lua::new();
        let message_handlers = Arc::new(Mutex::new(Vec::new()));
        let peer_refs = Arc::new(RwLock::new(HashMap::new()));
        let (stop_tx, stop_rx) = watch::channel(false);

        Ok(Self {
            client,
            updates,
            lua,
            message_handlers,
            peer_refs,
            console,
            stop_tx,
            stop_rx,
        })
    }

    /// Loads and evaluates a `.lua` bot script file, exposing global `ox` table helper functions.
    pub async fn load_script<P: AsRef<Path>>(&mut self, script_path: P) -> Result<(), OxideError> {
        let path = script_path.as_ref();
        let content =
            tokio::fs::read_to_string(path)
                .await
                .map_err(|source| ScriptError::ScriptIo {
                    path: path.to_path_buf(),
                    source,
                })?;

        let client = self.client.clone();
        let handlers = Arc::clone(&self.message_handlers);
        let peer_refs = Arc::clone(&self.peer_refs);

        let globals = self.lua.globals();
        let ox_table = self.lua.create_table().map_err(ScriptError::LuaError)?;

        // ox.send_message(chat_id, text, delay_sec)
        let client_clone = client.clone();
        let send_peer_refs = Arc::clone(&peer_refs);
        let send_msg_fn = self
            .lua
            .create_async_function(
                move |_, (chat_id, text, delay): (i64, String, Option<f64>)| {
                    let client = client_clone.clone();
                    let peer_refs = Arc::clone(&send_peer_refs);
                    async move {
                        if let Some(d) = delay
                            && d > 0.0
                        {
                            tokio::time::sleep(std::time::Duration::from_secs_f64(d)).await;
                        }

                        let peer_ref = peer_refs
                            .read()
                            .await
                            .get(&chat_id)
                            .copied()
                            .ok_or_else(|| {
                                mlua::Error::RuntimeError(format!(
                                    "Telegram peer {chat_id} is unknown; receive a message from it first"
                                ))
                            })?;

                        client.send_message(peer_ref, text).await.map_err(|e| {
                            mlua::Error::RuntimeError(format!("Telegram send error: {e}"))
                        })?;

                        Ok(())
                    }
                },
            )
            .map_err(ScriptError::LuaError)?;

        ox_table
            .set("send_message", send_msg_fn.clone())
            .map_err(ScriptError::LuaError)?;

        // ox.reply(chat_id, text, delay_sec)
        ox_table
            .set("reply", send_msg_fn)
            .map_err(ScriptError::LuaError)?;

        // ox.send_image(chat_id, path, optional_caption, optional_delay_sec)
        let client_clone = client.clone();
        let image_peer_refs = Arc::clone(&peer_refs);
        let send_image_fn = self
            .lua
            .create_async_function(
                move |_,
                      (chat_id, path, caption, delay): (
                    i64,
                    String,
                    Option<String>,
                    Option<f64>,
                )| {
                    let client = client_clone.clone();
                    let peer_refs = Arc::clone(&image_peer_refs);
                    async move {
                        if let Some(delay) = delay
                            && delay > 0.0
                        {
                            tokio::time::sleep(std::time::Duration::from_secs_f64(delay)).await;
                        }

                        let uploaded = client.upload_file(&path).await.map_err(|error| {
                            mlua::Error::RuntimeError(format!(
                                "Failed to upload image '{path}': {error}"
                            ))
                        })?;
                        let message = InputMessage::new()
                            .text(caption.unwrap_or_default())
                            .photo(uploaded);
                        let peer_ref = peer_refs
                            .read()
                            .await
                            .get(&chat_id)
                            .copied()
                            .ok_or_else(|| {
                                mlua::Error::RuntimeError(format!(
                                    "Telegram peer {chat_id} is unknown; receive a message from it first"
                                ))
                            })?;

                        client
                            .send_message(peer_ref, message)
                            .await
                            .map_err(|error| {
                                mlua::Error::RuntimeError(format!(
                                    "Telegram image send error: {error}"
                                ))
                            })?;
                        Ok(())
                    }
                },
            )
            .map_err(ScriptError::LuaError)?;
        ox_table
            .set("send_image", send_image_fn)
            .map_err(ScriptError::LuaError)?;

        // ox.input(prompt, optional_default)
        let console = Arc::clone(&self.console);
        let input_fn = self
            .lua
            .create_function(move |_, (prompt, default): (String, Option<String>)| {
                console
                    .ask_input(&prompt, default.as_deref())
                    .map_err(mlua::Error::RuntimeError)
            })
            .map_err(ScriptError::LuaError)?;
        ox_table
            .set("input", input_fn)
            .map_err(ScriptError::LuaError)?;

        // ox.stop()
        let stop_fn =
            create_stop_function(&self.lua, self.stop_tx.clone()).map_err(ScriptError::LuaError)?;
        ox_table
            .set("stop", stop_fn)
            .map_err(ScriptError::LuaError)?;

        // ox.on_message(filters_or_func, optional_func)
        let register_handler_fn = self
            .lua
            .create_function(move |lua_ctx, (arg1, arg2): (Value, Option<Function>)| {
                let (filter, func) = match (arg1, arg2) {
                    (Value::Table(t), Some(f)) => {
                        let parsed_filter = parse_message_filter(&t)?;
                        (Some(parsed_filter), f)
                    }
                    (Value::Function(f), _) => (None, f),
                    _ => {
                        return Err(mlua::Error::RuntimeError(
                            "Invalid arguments for ox.on_message".into(),
                        ));
                    }
                };

                let key = lua_ctx.create_registry_value(func)?;
                handlers
                    .try_lock()
                    .map_err(|_| {
                        mlua::Error::RuntimeError(
                            "Cannot register a message handler while handlers are running".into(),
                        )
                    })?
                    .push(RegisteredHandler {
                        callback_key: key,
                        filter,
                    });
                Ok(())
            })
            .map_err(ScriptError::LuaError)?;

        ox_table
            .set("on_message", register_handler_fn)
            .map_err(ScriptError::LuaError)?;

        globals
            .set("ox", ox_table.clone())
            .map_err(ScriptError::LuaError)?;
        globals
            .set("nox", ox_table)
            .map_err(ScriptError::LuaError)?;

        self.lua
            .load(&content)
            .exec()
            .map_err(ScriptError::LuaError)?;
        let handler_count = self.message_handlers.lock().await.len();
        info!(path = %path.display(), handler_count, "Lua script loaded");
        Ok(())
    }

    /// Starts listening for incoming MTProto updates and invokes registered Lua handlers matching filters.
    ///
    /// Returns when the update stream ends, Lua calls `ox.stop()`, or the user presses Ctrl+C.
    pub async fn run_event_loop(self) -> Result<(), OxideError> {
        info!(
            handler_count = self.message_handlers.lock().await.len(),
            "Bot event loop started"
        );
        let updates = self.updates;
        let mut stop_rx = self.stop_rx;
        let mut stream = self
            .client
            .stream_updates(updates, UpdatesConfiguration::default())
            .await
            .map_err(|e| ScriptError::UpdateStream(e.to_string()))?;
        let ctrl_c = tokio::signal::ctrl_c();
        tokio::pin!(ctrl_c);

        loop {
            if *stop_rx.borrow() {
                info!("Bot event loop stopped by Lua script");
                break;
            }

            let update = tokio::select! {
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
                    break;
                }
                next = stream.next() => next,
            };

            let Ok(update) = update else {
                warn!("Telegram update stream ended");
                break;
            };
            if let Update::NewMessage(message) = update {
                let handlers = self.message_handlers.lock().await;
                if handlers.is_empty() {
                    continue;
                }

                let text = message.text().to_string();
                let Some(chat_id) = message.peer_id().bare_id() else {
                    warn!("Skipping message with unresolved chat ID");
                    continue;
                };
                let peer_ref = match message.peer_ref().await {
                    Ok(Some(peer_ref)) => peer_ref,
                    Ok(None) => {
                        warn!(chat_id, "Skipping message with unresolved Telegram peer");
                        continue;
                    }
                    Err(error) => {
                        warn!(chat_id, error = %error, "Failed to resolve Telegram peer");
                        continue;
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
                let is_private = chat_type == ChatType::Private;
                let is_group = chat_type == ChatType::Group;
                let is_channel = chat_type == ChatType::Channel;
                debug!(
                    chat_id,
                    sender_id,
                    incoming,
                    text = %text,
                    "Telegram message received"
                );

                let event_table = match self.lua.create_table() {
                    Ok(t) => t,
                    Err(e) => {
                        error!("Failed to create Lua event table: {}", e);
                        continue;
                    }
                };

                event_table
                    .set("text", text.clone())
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
                    .set("is_private", is_private)
                    .map_err(ScriptError::LuaError)?;
                event_table
                    .set("is_group", is_group)
                    .map_err(ScriptError::LuaError)?;
                event_table
                    .set("is_channel", is_channel)
                    .map_err(ScriptError::LuaError)?;

                // event:reply(text, delay) method
                let client_clone = self.client.clone();
                let reply_fn = match self.lua.create_async_function(
                    move |_, (reply_text, delay): (String, Option<f64>)| {
                        let client = client_clone.clone();
                        async move {
                            if let Some(d) = delay
                                && d > 0.0
                            {
                                tokio::time::sleep(std::time::Duration::from_secs_f64(d)).await;
                            }

                            client
                                .send_message(peer_ref, reply_text)
                                .await
                                .map_err(|e| {
                                    mlua::Error::RuntimeError(format!("Telegram send error: {e}"))
                                })?;

                            Ok(())
                        }
                    },
                ) {
                    Ok(f) => f,
                    Err(e) => {
                        error!("Failed to bind event:reply in Lua: {}", e);
                        continue;
                    }
                };

                event_table
                    .set("reply", reply_fn)
                    .map_err(ScriptError::LuaError)?;

                for item in handlers.iter() {
                    if let Some(ref filter) = item.filter {
                        let context = MessageContext {
                            text: &text,
                            chat_id,
                            sender_id,
                            incoming,
                            chat_type,
                        };
                        if !filter.matches(&context) {
                            debug!(chat_id, sender_id, "Lua message filter did not match");
                            continue;
                        }
                    }

                    debug!(chat_id, sender_id, "Running Lua message handler");
                    if let Ok(func) = self.lua.registry_value::<Function>(&item.callback_key)
                        && let Err(err) = func.call_async::<()>(event_table.clone()).await
                    {
                        error!(chat_id, sender_id, error = %err, "Lua message handler failed");
                    }
                    if *stop_rx.borrow() {
                        break;
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{create_stop_function, parse_message_filter};
    use crate::domain::automation::{ChatType, MessageContext};
    use mlua::{Function, Lua, Table, Value};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn parses_and_matches_combined_filters() {
        let lua = Lua::new();
        let table = lua
            .load(
                r#"return {
                    chats = { 100, 200 },
                    senders = 42,
                    incoming = true,
                    private = true,
                    has_text = true,
                    commands = { "start", "/help" },
                    pattern = "^/start"
                }"#,
            )
            .eval()
            .unwrap();
        let filter = parse_message_filter(&table).unwrap();

        assert!(filter.matches(&MessageContext {
            text: "/start payload",
            chat_id: 100,
            sender_id: 42,
            incoming: true,
            chat_type: ChatType::Private,
        }));
        assert!(!filter.matches(&MessageContext {
            text: "/start payload",
            chat_id: 300,
            sender_id: 42,
            incoming: true,
            chat_type: ChatType::Private,
        }));
        assert!(!filter.matches(&MessageContext {
            text: "/start payload",
            chat_id: 100,
            sender_id: 42,
            incoming: false,
            chat_type: ChatType::Private,
        }));
    }

    #[test]
    fn command_filter_supports_bot_suffix_and_case_insensitive_names() {
        let lua = Lua::new();
        let table = lua.load(r#"return { commands = "START" }"#).eval().unwrap();
        let filter = parse_message_filter(&table).unwrap();

        assert!(filter.matches(&MessageContext {
            text: "/start@my_bot argument",
            chat_id: 1,
            sender_id: 1,
            incoming: true,
            chat_type: ChatType::Group,
        }));
    }

    #[test]
    fn rejects_invalid_filter_types_and_patterns() {
        let lua = Lua::new();
        let invalid_type = lua.load(r#"return { chats = "100" }"#).eval().unwrap();
        let invalid_pattern = lua.load(r#"return { pattern = "[" }"#).eval().unwrap();

        assert!(parse_message_filter(&invalid_type).is_err());
        assert!(parse_message_filter(&invalid_pattern).is_err());
    }

    #[test]
    fn anonchat_script_loads_with_startup_input_api() {
        let lua = Lua::new();
        let ox = lua.create_table().unwrap();
        let input = lua
            .create_function(
                |_, (prompt, _): (String, Option<String>)| match prompt.as_str() {
                    "Enable premium search? [y/N]" => Ok("n"),
                    "Spam mode (text/photo/combined)" => Ok("text"),
                    "Message to send" => Ok("hello"),
                    _ => Err(mlua::Error::RuntimeError(format!(
                        "unexpected prompt: {prompt}"
                    ))),
                },
            )
            .unwrap();
        let handlers = Arc::new(AtomicUsize::new(0));
        let handler_count = Arc::clone(&handlers);
        let on_message = lua
            .create_function(move |_, (_filter, _callback): (Table, Function)| {
                handler_count.fetch_add(1, Ordering::Relaxed);
                Ok(())
            })
            .unwrap();

        ox.set("input", input).unwrap();
        ox.set("on_message", on_message).unwrap();
        ox.set(
            "send_message",
            lua.create_function(|_, _: Value| Ok(())).unwrap(),
        )
        .unwrap();
        ox.set(
            "send_image",
            lua.create_function(|_, _: Value| Ok(())).unwrap(),
        )
        .unwrap();
        lua.globals().set("ox", ox).unwrap();

        lua.load(include_str!("../../data/bots/anonchat.lua"))
            .exec()
            .unwrap();
        assert_eq!(handlers.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn async_lua_api_runs_inside_message_handler() {
        let lua = Lua::new();
        let calls = Arc::new(AtomicUsize::new(0));
        let call_count = Arc::clone(&calls);
        let send = lua
            .create_async_function(move |_, ()| {
                let call_count = Arc::clone(&call_count);
                async move {
                    call_count.fetch_add(1, Ordering::Relaxed);
                    Ok(())
                }
            })
            .unwrap();
        lua.globals().set("send", send).unwrap();
        let handler: Function = lua.load("return function() send() end").eval().unwrap();

        handler.call_async::<()>(()).await.unwrap();

        assert_eq!(calls.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn stop_api_requests_idempotent_shutdown() {
        let lua = Lua::new();
        let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
        let stop = create_stop_function(&lua, stop_tx).unwrap();
        lua.globals().set("stop", stop).unwrap();

        lua.load("stop(); stop()").exec().unwrap();

        assert!(*stop_rx.borrow());
    }
}
