//! Lua bot runtime environment built on top of `mlua` and Tokio async tasks.
//!
//! Exposes helper functions (`ox.send_message`, `ox.reply`, `ox.on_message`) to Lua bot scripts,
//! supports Telethon-like filters for peers, direction, chat type, text, commands, and patterns,
//! listens for MTProto updates, and dispatches matching messages to registered Lua callbacks.

use crate::errors::{OxideError, ScriptError};
use grammers_client::Client;
use grammers_client::client::UpdatesConfiguration;
use grammers_client::update::Update;
use grammers_session::types::{PeerAuth, PeerId, PeerKind, PeerRef};
use grammers_session::updates::UpdatesLike;
use mlua::{Function, Lua, RegistryKey, Table, Value};
use regex::Regex;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::sync::{Mutex, watch};
use tracing::{error, info, warn};

/// Message filter options for fine-grained event handling (Telethon-like).
#[derive(Debug, Clone, Default)]
pub struct MessageFilter {
    pub chats: Option<Vec<i64>>,
    pub senders: Option<Vec<i64>>,
    pub incoming: Option<bool>,
    pub outgoing: Option<bool>,
    pub private: Option<bool>,
    pub group: Option<bool>,
    pub channel: Option<bool>,
    pub has_text: Option<bool>,
    pub commands: Option<Vec<String>>,
    pub pattern: Option<Regex>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatType {
    Private,
    Group,
    Channel,
}

#[derive(Debug)]
pub struct MessageContext<'a> {
    pub text: &'a str,
    pub chat_id: i64,
    pub sender_id: i64,
    pub incoming: bool,
    pub chat_type: ChatType,
}

impl MessageFilter {
    /// Parses filter options from a Lua table.
    pub fn from_lua_table(table: &Table) -> Result<Self, mlua::Error> {
        let mut filter = Self::default();

        filter.chats = parse_integer_list(table, "chats")?;
        filter.senders = parse_integer_list(table, "senders")?;
        filter.incoming = parse_boolean(table, "incoming")?;
        filter.outgoing = parse_boolean(table, "outgoing")?;
        filter.private = parse_boolean(table, "private")?;
        filter.group = parse_boolean(table, "group")?;
        filter.channel = parse_boolean(table, "channel")?;
        filter.has_text = parse_boolean(table, "has_text")?;
        filter.commands = parse_string_list(table, "commands")?.map(|commands| {
            commands
                .into_iter()
                .map(|command| command.trim_start_matches('/').to_lowercase())
                .collect()
        });

        if let Some(pattern) = parse_string(table, "pattern")? {
            filter.pattern = Some(Regex::new(&pattern).map_err(|error| {
                mlua::Error::RuntimeError(format!("invalid 'pattern' regex '{pattern}': {error}"))
            })?);
        }

        Ok(filter)
    }

    /// Evaluates if an incoming message matches the filter rules.
    pub fn matches(&self, context: &MessageContext<'_>) -> bool {
        if let Some(ref chats) = self.chats {
            if !chats.contains(&context.chat_id) {
                return false;
            }
        }

        if let Some(ref senders) = self.senders {
            if !senders.contains(&context.sender_id) {
                return false;
            }
        }

        if let Some(inc) = self.incoming {
            if inc != context.incoming {
                return false;
            }
        }

        if let Some(out) = self.outgoing {
            if out != !context.incoming {
                return false;
            }
        }

        if self
            .private
            .is_some_and(|expected| expected != (context.chat_type == ChatType::Private))
            || self
                .group
                .is_some_and(|expected| expected != (context.chat_type == ChatType::Group))
            || self
                .channel
                .is_some_and(|expected| expected != (context.chat_type == ChatType::Channel))
            || self
                .has_text
                .is_some_and(|expected| expected != !context.text.is_empty())
        {
            return false;
        }

        if let Some(commands) = &self.commands {
            let command = context
                .text
                .strip_prefix('/')
                .and_then(|text| text.split_whitespace().next())
                .and_then(|text| text.split('@').next())
                .map(str::to_lowercase);
            if !command.is_some_and(|command| commands.contains(&command)) {
                return false;
            }
        }

        if self
            .pattern
            .as_ref()
            .is_some_and(|regex| !regex.is_match(context.text))
        {
            return false;
        }

        true
    }
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
}

impl LuaBotRunner {
    /// Creates a new `LuaBotRunner` with an authorized Telegram client and updates channel.
    ///
    pub fn new(
        client: Client,
        updates: mpsc::UnboundedReceiver<UpdatesLike>,
    ) -> Result<Self, OxideError> {
        let lua = Lua::new();
        let message_handlers = Arc::new(Mutex::new(Vec::new()));

        Ok(Self {
            client,
            updates,
            lua,
            message_handlers,
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

        let globals = self.lua.globals();
        let ox_table = self.lua.create_table().map_err(ScriptError::LuaError)?;

        // ox.send_message(chat_id, text, delay_sec)
        let client_clone = client.clone();
        let send_msg_fn = self
            .lua
            .create_async_function(
                move |_, (chat_id, text, delay): (i64, String, Option<f64>)| {
                    let client = client_clone.clone();
                    async move {
                        if let Some(d) = delay {
                            if d > 0.0 {
                                tokio::time::sleep(std::time::Duration::from_secs_f64(d)).await;
                            }
                        }

                        let peer_id = PeerId::user_unchecked(chat_id);
                        let peer_ref = PeerRef {
                            id: peer_id,
                            auth: PeerAuth::from_hash(0),
                        };

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

        // ox.on_message(filters_or_func, optional_func)
        let register_handler_fn = self
            .lua
            .create_function(move |lua_ctx, (arg1, arg2): (Value, Option<Function>)| {
                let (filter, func) = match (arg1, arg2) {
                    (Value::Table(t), Some(f)) => {
                        let parsed_filter = MessageFilter::from_lua_table(&t)?;
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
                let handlers = Arc::clone(&handlers);
                tokio::spawn(async move {
                    handlers.lock().await.push(RegisteredHandler {
                        callback_key: key,
                        filter,
                    });
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
    /// Returns when the updates stream ends or when `cancel_rx` signals cancellation.
    pub async fn run_event_loop(
        self,
        mut cancel_rx: watch::Receiver<bool>,
    ) -> Result<(), OxideError> {
        info!(
            handler_count = self.message_handlers.lock().await.len(),
            "Bot event loop started"
        );
        let updates = self.updates;
        let mut stream = self
            .client
            .stream_updates(updates, UpdatesConfiguration::default())
            .await
            .map_err(|e| ScriptError::UpdateStream(e.to_string()))?;

        loop {
            if *cancel_rx.borrow() {
                info!("Bot event loop cancelled.");
                break;
            }

            let update = tokio::select! {
                biased;
                changed = cancel_rx.changed() => {
                    if changed.is_ok() && *cancel_rx.borrow() {
                        info!("Bot event loop cancelled.");
                        break;
                    }
                    continue;
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
                // Skip messages without a resolved peer.
                if message.peer().is_none() {
                    continue;
                }

                let Some(chat_id) = message.peer_id().bare_id() else {
                    warn!("Skipping message with unresolved chat ID");
                    continue;
                };
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
                            if let Some(d) = delay {
                                if d > 0.0 {
                                    tokio::time::sleep(std::time::Duration::from_secs_f64(d)).await;
                                }
                            }

                            let peer_id = PeerId::user_unchecked(chat_id);
                            let peer_ref = PeerRef {
                                id: peer_id,
                                auth: PeerAuth::from_hash(0),
                            };

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
                            continue;
                        }
                    }

                    if let Ok(func) = self.lua.registry_value::<Function>(&item.callback_key) {
                        if let Err(err) = func.call::<()>(event_table.clone()) {
                            error!(chat_id, sender_id, error = %err, "Lua message handler failed");
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{ChatType, MessageContext, MessageFilter};
    use mlua::Lua;

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
        let filter = MessageFilter::from_lua_table(&table).unwrap();

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
        let filter = MessageFilter::from_lua_table(&table).unwrap();

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

        assert!(MessageFilter::from_lua_table(&invalid_type).is_err());
        assert!(MessageFilter::from_lua_table(&invalid_pattern).is_err());
    }
}
