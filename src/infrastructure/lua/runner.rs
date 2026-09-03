//! Lua bot runner managing script evaluation, update streaming, and event dispatching.

use crate::application::automation::BotConsole;
use crate::domain::automation::{ChatType, MessageContext};
use crate::errors::{OxideError, ScriptError};
use crate::infrastructure::lua::api::{RegisteredHandler, register_ox_table, resolve_peer_ref};
use grammers_client::Client;
use grammers_client::message::InputMessage;
use grammers_client::update::Update;
use grammers_mtsender::UpdatesConfiguration;
use grammers_session::types::{PeerKind, PeerRef};
use grammers_session::updates::UpdatesLike;
use mlua::{Function, Lua};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::sync::{Mutex, RwLock, watch};
use tracing::{debug, error, info, warn};

/// Runner responsible for binding Telegram methods to Lua, loading bot scripts, and handling updates.
pub struct LuaBotRunner {
    pub client: Client,
    pub updates: mpsc::Receiver<UpdatesLike>,
    pub lua: Lua,
    pub message_handlers: Arc<Mutex<Vec<RegisteredHandler>>>,
    peer_refs: Arc<RwLock<HashMap<i64, PeerRef>>>,
    console: Arc<dyn BotConsole>,
    stop_tx: watch::Sender<bool>,
    stop_rx: watch::Receiver<bool>,
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

        register_ox_table(
            &self.lua,
            self.client.clone(),
            Arc::clone(&self.message_handlers),
            Arc::clone(&self.peer_refs),
            Arc::clone(&self.console),
            self.stop_tx.clone(),
        )?;

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
                                warn!(error = %error, "Telegram update stream ended");
                                break;
                            }
                        }
                    }
                }
            }

            // Synchronize updates state with session before shutdown
            if let Err(error) = stream.sync_update_state().await {
                warn!(error = %error, "Failed to synchronize update state with session");
            }
        });

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
                    self.stop_tx.send_replace(true);
                    break;
                }
                update = update_rx.recv() => update,
            };

            let Some(update) = update else {
                warn!("Update receiver channel closed");
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
                    Ok(None) => match resolve_peer_ref(&self.client, &self.peer_refs, chat_id).await {
                        Ok(peer_ref) => peer_ref,
                        Err(_) => {
                            warn!(chat_id, "Skipping message with unresolved Telegram peer");
                            continue;
                        }
                    },
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

                // event:reply(text, delay) method quoting the original message
                let client_clone = self.client.clone();
                let message_id = message.id();
                let reply_fn = match self.lua.create_async_function(
                    move |_, (reply_text, delay): (String, Option<f64>)| {
                        let client = client_clone.clone();
                        async move {
                            if let Some(d) = delay
                                && d > 0.0
                            {
                                tokio::time::sleep(std::time::Duration::from_secs_f64(d)).await;
                            }

                            let reply_message = InputMessage::new()
                                .text(reply_text)
                                .reply_to(Some(message_id));

                            client
                                .send_message(peer_ref, reply_message)
                                .await
                                .map_err(|e| {
                                    mlua::Error::RuntimeError(format!("Telegram reply error: {e}"))
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

        self.stop_tx.send_replace(true);
        let _ = reader_handle.await;
        Ok(())
    }
}
