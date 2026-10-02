//! Interactive Dry-Run Bot Simulator for testing bot scripts without Telegram MTProto connections.

use crate::application::automation::BotConsole;
use crate::domain::automation::{
    BotButton, ButtonKind, ChatType, FilterMatchResult, KeyboardRow, MessageContext, MessageFilter,
    MessageMarkup,
};
use crate::domain::types::{ChatId, SenderId};
use crate::errors::{OxideError, ScriptError};
use crate::infrastructure::IO_TIMEOUT;
use crate::infrastructure::lua::api::{
    extract_log_msg, parse_click_button_args, parse_confirm_args, parse_delete_message_args,
    parse_edit_message_args, parse_file_args, parse_forward_args, parse_media_send_args,
    parse_pin_args, parse_react_args, parse_select_args, parse_select_options,
    parse_send_message_args,
};
use crate::infrastructure::lua::buttons::markup_to_lua_table;
use crate::infrastructure::lua::filters::parse_message_filter;
use crate::infrastructure::lua::storage::BotStorage;
use mlua::{Function, Lua, RegistryKey, Table, Value};
use notify::{EventKind, RecursiveMode, Watcher};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock, watch};
use tracing::info;

/// Registered message handler inside the simulator.
struct SimHandler {
    callback_key: RegistryKey,
    filter: Option<MessageFilter>,
}

/// Simulator engine managing mock event dispatching and interactive terminal REPL.
pub struct BotSimulator {
    lua: Lua,
    script_path: PathBuf,
    handlers: Arc<Mutex<Vec<SimHandler>>>,
    active_markup: Arc<RwLock<MessageMarkup>>,
    active_chat_id: Arc<RwLock<i64>>,
    stop_tx: watch::Sender<bool>,
    stop_rx: watch::Receiver<bool>,
    console: Arc<dyn BotConsole>,
    storage: BotStorage,
}

impl BotSimulator {
    /// Creates a new `BotSimulator` for the target `.lua` script file.
    pub async fn new<P: AsRef<Path>>(
        script_path: P,
        console: Arc<dyn BotConsole>,
    ) -> Result<Self, OxideError> {
        let path = script_path.as_ref().to_path_buf();
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("sim_default");

        let storage = BotStorage::open("data/storage", &format!("sim_{stem}")).await;
        let (stop_tx, stop_rx) = watch::channel(false);

        Ok(Self {
            lua: Lua::new(),
            script_path: path,
            handlers: Arc::new(Mutex::new(Vec::new())),
            active_markup: Arc::new(RwLock::new(MessageMarkup::default())),
            active_chat_id: Arc::new(RwLock::new(6015596466)),
            stop_tx,
            stop_rx,
            console,
            storage,
        })
    }

    /// Loads the bot script and registers mock `ox.*` APIs.
    pub async fn load_script(&mut self) -> Result<(), OxideError> {
        let content =
            tokio::time::timeout(IO_TIMEOUT, tokio::fs::read_to_string(&self.script_path))
                .await
                .map_err(|_| ScriptError::Timeout("Read script timed out".into()))?
                .map_err(|source| ScriptError::ScriptIo {
                    path: self.script_path.clone(),
                    source,
                })?;

        self.setup_mock_ox_table()?;

        self.lua
            .load(&content)
            .exec()
            .map_err(ScriptError::LuaError)?;

        let count = self.handlers.lock().await.len();
        info!(
            path = %self.script_path.display(),
            handler_count = count,
            "Bot script loaded into simulator"
        );
        Ok(())
    }

    /// Sets up the global `ox` table with mock bindings.
    fn setup_mock_ox_table(&self) -> Result<(), OxideError> {
        let ox = self.lua.create_table().map_err(ScriptError::LuaError)?;

        // Stdlib string methods & random helpers
        crate::infrastructure::lua::stdlib::register_stdlib(&self.lua, Some(&ox))
            .map_err(ScriptError::LuaError)?;
        crate::infrastructure::lua::stdlib::register_random_helpers(&self.lua, &ox)
            .map_err(ScriptError::LuaError)?;
        crate::infrastructure::lua::storage::register_storage_api(
            &self.lua,
            &ox,
            self.storage.clone(),
        )
        .map_err(ScriptError::LuaError)?;

        // ox.send_message & ox.reply
        let send_fn = self
            .lua
            .create_async_function(
                |_, (arg1, arg2, arg3): (Value, Option<Value>, Option<Value>)| async move {
                    let (chat_id, text, opts) = parse_send_message_args(arg1, arg2, arg3)?;
                    if let Some(d) = opts.delay {
                        println!("  \x1b[32m[BOT SEND]\x1b[0m Chat: {chat_id} | Text: \"{text}\" (delay: {d:.1}s)");
                    } else {
                        println!("  \x1b[32m[BOT SEND]\x1b[0m Chat: {chat_id} | Text: \"{text}\"");
                    }
                    Ok(())
                },
            )
            .map_err(ScriptError::LuaError)?;
        ox.set("send_message", send_fn)
            .map_err(ScriptError::LuaError)?;

        // ox.send_image
        let image_fn = self
            .lua
            .create_function(
                |_, (a1, a2, a3, a4): (Value, Option<Value>, Option<Value>, Option<Value>)| {
                    let (chat_id, path, caption, _) =
                        parse_media_send_args("send_image", a1, a2, a3, a4)?;
                    let cap = caption.unwrap_or_default();
                    println!("  \x1b[32m[BOT IMAGE]\x1b[0m Chat: {chat_id} | Path: \"{path}\" | Caption: \"{cap}\"");
                    Ok(())
                },
            )
            .map_err(ScriptError::LuaError)?;
        ox.set("send_image", image_fn)
            .map_err(ScriptError::LuaError)?;

        // ox.send_document
        let doc_fn = self
            .lua
            .create_function(
                |_, (a1, a2, a3, a4): (Value, Option<Value>, Option<Value>, Option<Value>)| {
                    let (chat_id, path, caption, _) =
                        parse_media_send_args("send_document", a1, a2, a3, a4)?;
                    let cap = caption.unwrap_or_default();
                    println!("  \x1b[32m[BOT DOC]\x1b[0m Chat: {chat_id} | Path: \"{path}\" | Caption: \"{cap}\"");
                    Ok(())
                },
            )
            .map_err(ScriptError::LuaError)?;
        ox.set("send_document", doc_fn)
            .map_err(ScriptError::LuaError)?;

        // ox.send_audio
        let audio_fn = self
            .lua
            .create_function(
                |_, (a1, a2, a3, a4): (Value, Option<Value>, Option<Value>, Option<Value>)| {
                    let (chat_id, path, caption, _) =
                        parse_media_send_args("send_audio", a1, a2, a3, a4)?;
                    let cap = caption.unwrap_or_default();
                    println!("  \x1b[32m[BOT AUDIO]\x1b[0m Chat: {chat_id} | Path: \"{path}\" | Caption: \"{cap}\"");
                    Ok(())
                },
            )
            .map_err(ScriptError::LuaError)?;
        ox.set("send_audio", audio_fn)
            .map_err(ScriptError::LuaError)?;

        // ox.send_voice
        let voice_fn = self
            .lua
            .create_function(|_, (a1, a2, a3): (Value, Option<Value>, Option<Value>)| {
                let (chat_id, path, _, _) = parse_media_send_args("send_voice", a1, a2, None, a3)?;
                println!("  \x1b[32m[BOT VOICE]\x1b[0m Chat: {chat_id} | Path: \"{path}\"");
                Ok(())
            })
            .map_err(ScriptError::LuaError)?;
        ox.set("send_voice", voice_fn)
            .map_err(ScriptError::LuaError)?;

        // ox.click_button
        let click_btn_fn = self
            .lua
            .create_function(
                |lua, (arg1, arg2, arg3): (Value, Option<Value>, Option<Value>)| {
                    let (chat_id, msg_id, data) = parse_click_button_args(arg1, arg2, arg3)?;
                    println!("  \x1b[33m[BOT CLICK_BUTTON]\x1b[0m Chat: {chat_id} | Msg: {msg_id} | Data: \"{data}\"");
                    let res = lua.create_table()?;
                    res.set("message", "Simulated button callback answer")?;
                    Ok(res)
                },
            )
            .map_err(ScriptError::LuaError)?;
        ox.set("click_button", click_btn_fn)
            .map_err(ScriptError::LuaError)?;

        // ox.edit_message
        let edit_fn = self
            .lua
            .create_function(
                |_, (a1, a2, a3, a4): (Value, Option<Value>, Option<Value>, Option<Value>)| {
                    let (chat_id, msg_id, new_text, _) =
                        parse_edit_message_args(a1, a2, a3, a4)?;
                    println!("  \x1b[36m[BOT EDIT]\x1b[0m Chat: {chat_id} | Msg: {msg_id} | Text: \"{new_text}\"");
                    Ok(())
                },
            )
            .map_err(ScriptError::LuaError)?;
        ox.set("edit_message", edit_fn)
            .map_err(ScriptError::LuaError)?;

        // ox.delete_message
        let del_fn = self
            .lua
            .create_function(
                |_, (arg1, arg2, arg3): (Value, Option<Value>, Option<Value>)| {
                    let (chat_id, ids, _) = parse_delete_message_args(arg1, arg2, arg3)?;
                    println!(
                        "  \x1b[31m[BOT DELETE]\x1b[0m Chat: {chat_id} | Messages deleted: {ids:?}"
                    );
                    Ok(())
                },
            )
            .map_err(ScriptError::LuaError)?;
        ox.set("delete_message", del_fn)
            .map_err(ScriptError::LuaError)?;

        // ox.react
        let react_fn = self
            .lua
            .create_function(
                |_, (a1, a2, a3, a4): (Value, Option<Value>, Option<Value>, Option<Value>)| {
                    let (chat_id, msg_id, emoji, _) = parse_react_args(a1, a2, a3, a4)?;
                    println!(
                        "  \x1b[35m[BOT REACT]\x1b[0m Chat: {chat_id} | Msg: {msg_id} | Emoji: {emoji}"
                    );
                    Ok(())
                },
            )
            .map_err(ScriptError::LuaError)?;
        ox.set("react", react_fn).map_err(ScriptError::LuaError)?;

        // ox.pin_message
        let pin_fn = self
            .lua
            .create_function(
                |_, (arg1, arg2, arg3): (Value, Option<Value>, Option<Value>)| {
                    let (chat_id, msg_id, _) = parse_pin_args(arg1, arg2, arg3)?;
                    println!("  \x1b[35m[BOT PIN]\x1b[0m Chat: {chat_id} | Msg: {msg_id}");
                    Ok(())
                },
            )
            .map_err(ScriptError::LuaError)?;
        ox.set("pin_message", pin_fn)
            .map_err(ScriptError::LuaError)?;

        // ox.forward_message
        let fwd_fn = self
            .lua
            .create_function(
                |_, (arg1, arg2, arg3): (Value, Option<Value>, Option<Value>)| {
                    let (to_chat_id, from_chat_id, msg_id) =
                        parse_forward_args(arg1, arg2, arg3)?;
                    println!("  \x1b[34m[BOT FORWARD]\x1b[0m To: {to_chat_id} | From: {from_chat_id} | Msg: {msg_id}");
                    Ok(())
                },
            )
            .map_err(ScriptError::LuaError)?;
        ox.set("forward_message", fwd_fn)
            .map_err(ScriptError::LuaError)?;

        // ox.select
        let console_clone = Arc::clone(&self.console);
        let select_fn = self
            .lua
            .create_function(
                move |_, (arg1, arg2, arg3): (Value, Option<Value>, Option<Value>)| {
                    let (prompt, options_val, default) = parse_select_args(arg1, arg2, arg3)?;
                    let (labels, values) = parse_select_options(&options_val)?;

                    let default_label = default.as_ref().and_then(|def| {
                        values
                            .iter()
                            .position(|v| v.eq_ignore_ascii_case(def))
                            .or_else(|| labels.iter().position(|l| l.eq_ignore_ascii_case(def)))
                            .map(|idx| labels[idx].clone())
                    });

                    let selected_label = console_clone
                        .ask_select(&prompt, labels.clone(), default_label.as_deref())
                        .map_err(mlua::Error::RuntimeError)?;

                    let idx = labels
                        .iter()
                        .position(|l| l == &selected_label)
                        .unwrap_or(0);
                    Ok(values[idx].clone())
                },
            )
            .map_err(ScriptError::LuaError)?;
        ox.set("select", select_fn).map_err(ScriptError::LuaError)?;

        // ox.confirm
        let console_clone = Arc::clone(&self.console);
        let confirm_fn = self
            .lua
            .create_function(move |_, (arg1, arg2): (Value, Option<bool>)| {
                let (prompt, default) = parse_confirm_args(arg1, arg2)?;
                console_clone
                    .ask_confirm(&prompt, default)
                    .map_err(mlua::Error::RuntimeError)
            })
            .map_err(ScriptError::LuaError)?;
        ox.set("confirm", confirm_fn)
            .map_err(ScriptError::LuaError)?;

        // ox.input
        // ox.input
        let console_clone = Arc::clone(&self.console);
        let input_fn = self
            .lua
            .create_function(move |lua, (arg1, second_arg): (Value, Option<Value>)| {
                let (prompt, opt_arg) = match arg1 {
                    Value::Table(ref t) if t.contains_key("prompt")? => {
                        let p: String = t.get("prompt")?;
                        let def: Option<String> = t.get("default").ok();
                        let def_val = match def {
                            Some(s) => Some(Value::String(lua.create_string(&s)?)),
                            None => None,
                        };
                        (p, def_val)
                    }
                    Value::String(s) => (s.to_str()?.to_string(), second_arg),
                    _ => {
                        return Err(mlua::Error::RuntimeError(
                            "input requires prompt string or table with 'prompt'".into(),
                        ));
                    }
                };

                let default_str = match opt_arg {
                    Some(Value::String(s)) => Some(
                        s.to_str()
                            .map_err(|e| mlua::Error::RuntimeError(e.to_string()))?
                            .to_string(),
                    ),
                    Some(Value::Integer(i)) => Some(i.to_string()),
                    Some(Value::Number(n)) => Some(n.to_string()),
                    _ => None,
                };
                console_clone
                    .ask_input(&prompt, default_str.as_deref())
                    .map_err(mlua::Error::RuntimeError)
            })
            .map_err(ScriptError::LuaError)?;
        ox.set("input", input_fn).map_err(ScriptError::LuaError)?;

        // ox.file
        let console_clone = Arc::clone(&self.console);
        let file_fn = self
            .lua
            .create_function(move |_, (arg1, arg2): (Value, Option<Value>)| {
                let (prompt, default, must_exist, extensions) = parse_file_args(arg1, arg2)?;
                console_clone
                    .ask_file(&prompt, default.as_deref(), must_exist, extensions)
                    .map_err(mlua::Error::RuntimeError)
            })
            .map_err(ScriptError::LuaError)?;
        ox.set("file", file_fn).map_err(ScriptError::LuaError)?;

        // ox.log
        let log_table = self.lua.create_table().map_err(ScriptError::LuaError)?;
        let log_info = self
            .lua
            .create_function(|_, (a1, a2): (Value, Option<Value>)| {
                let msg = extract_log_msg(a1, a2)?;
                println!("  \x1b[32m[LOG:INFO]\x1b[0m {msg}");
                Ok(())
            })
            .map_err(ScriptError::LuaError)?;
        let log_warn = self
            .lua
            .create_function(|_, (a1, a2): (Value, Option<Value>)| {
                let msg = extract_log_msg(a1, a2)?;
                println!("  \x1b[33m[LOG:WARN]\x1b[0m {msg}");
                Ok(())
            })
            .map_err(ScriptError::LuaError)?;
        let log_error = self
            .lua
            .create_function(|_, (a1, a2): (Value, Option<Value>)| {
                let msg = extract_log_msg(a1, a2)?;
                println!("  \x1b[31m[LOG:ERROR]\x1b[0m {msg}");
                Ok(())
            })
            .map_err(ScriptError::LuaError)?;
        let log_debug = self
            .lua
            .create_function(|_, (a1, a2): (Value, Option<Value>)| {
                let msg = extract_log_msg(a1, a2)?;
                println!("  \x1b[90m[LOG:DEBUG]\x1b[0m {msg}");
                Ok(())
            })
            .map_err(ScriptError::LuaError)?;
        log_table
            .set("info", log_info)
            .map_err(ScriptError::LuaError)?;
        log_table
            .set("warn", log_warn)
            .map_err(ScriptError::LuaError)?;
        log_table
            .set("error", log_error)
            .map_err(ScriptError::LuaError)?;
        log_table
            .set("debug", log_debug)
            .map_err(ScriptError::LuaError)?;
        ox.set("log", log_table).map_err(ScriptError::LuaError)?;

        // ox.send_typing
        let typing_fn = self
            .lua
            .create_function(|_, chat_id: i64| {
                println!("  \x1b[90m[BOT TYPING]\x1b[0m Chat: {chat_id}");
                Ok(())
            })
            .map_err(ScriptError::LuaError)?;
        ox.set("send_typing", typing_fn)
            .map_err(ScriptError::LuaError)?;

        // ox.set_interval / ox.set_timeout / ox.clear_timer
        let timer_fn = self
            .lua
            .create_function(|_, (secs, _cb): (f64, Function)| {
                println!("  \x1b[90m[BOT TIMER]\x1b[0m Registered timer for {secs:.1}s");
                Ok(1u64)
            })
            .map_err(ScriptError::LuaError)?;
        ox.set("set_interval", timer_fn.clone())
            .map_err(ScriptError::LuaError)?;
        ox.set("set_timeout", timer_fn)
            .map_err(ScriptError::LuaError)?;

        let clear_timer_fn = self
            .lua
            .create_function(|_, id: u64| {
                println!("  \x1b[90m[BOT TIMER]\x1b[0m Cleared timer {id}");
                Ok(true)
            })
            .map_err(ScriptError::LuaError)?;
        ox.set("clear_timer", clear_timer_fn.clone())
            .map_err(ScriptError::LuaError)?;
        ox.set("clear_interval", clear_timer_fn.clone())
            .map_err(ScriptError::LuaError)?;
        ox.set("clear_timeout", clear_timer_fn)
            .map_err(ScriptError::LuaError)?;

        // ox.sleep
        let sleep_fn = self
            .lua
            .create_async_function(|_, secs: f64| async move {
                println!("  \x1b[90m[BOT SLEEP]\x1b[0m Sleeping {secs:.1}s (clamped in simulator)");
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                Ok(())
            })
            .map_err(ScriptError::LuaError)?;
        ox.set("sleep", sleep_fn).map_err(ScriptError::LuaError)?;

        // ox.stop
        let stop_tx = self.stop_tx.clone();
        let stop_fn = self
            .lua
            .create_function(move |_, ()| {
                println!("  \x1b[31m[BOT STOP]\x1b[0m Script requested stop.");
                stop_tx.send_replace(true);
                Ok(())
            })
            .map_err(ScriptError::LuaError)?;
        ox.set("stop", stop_fn).map_err(ScriptError::LuaError)?;

        // ox.on_message
        let handlers_store = Arc::clone(&self.handlers);
        let on_msg_fn = self
            .lua
            .create_function(move |lua, (arg1, arg2): (Value, Option<Function>)| {
                let (filter, callback) = match (arg1, arg2) {
                    (Value::Table(t), Some(f)) => (Some(parse_message_filter(&t)?), f),
                    (Value::Function(f), _) => (None, f),
                    _ => {
                        return Err(mlua::Error::RuntimeError(
                            "Invalid arguments for ox.on_message".into(),
                        ));
                    }
                };
                let key = lua.create_registry_value(callback)?;
                if let Ok(mut list) = handlers_store.try_lock() {
                    list.push(SimHandler {
                        callback_key: key,
                        filter,
                    });
                }
                Ok(())
            })
            .map_err(ScriptError::LuaError)?;
        ox.set("on_message", on_msg_fn)
            .map_err(ScriptError::LuaError)?;

        // ox.flow constructor
        crate::infrastructure::lua::flow::register_flow_api(&self.lua, &ox)
            .map_err(ScriptError::LuaError)?;

        let globals = self.lua.globals();
        globals.set("ox", ox).map_err(ScriptError::LuaError)?;

        Ok(())
    }

    /// Dispatches a simulated incoming text message to all matching Lua handlers.
    pub async fn simulate_incoming(&self, text: &str) -> Result<(), OxideError> {
        let chat_id = *self.active_chat_id.read().await;
        let markup = self.active_markup.read().await.clone();

        let context = MessageContext {
            text,
            chat_id: ChatId::new(chat_id),
            sender_id: SenderId::new(chat_id),
            incoming: true,
            chat_type: ChatType::Private,
        };

        let handlers = self.handlers.lock().await;
        for handler in handlers.iter() {
            let match_result = if let Some(ref filter) = handler.filter {
                match filter.match_context(&context) {
                    Some(res) => res,
                    None => continue,
                }
            } else {
                FilterMatchResult::default()
            };

            let event = self.create_sim_event(text, chat_id, &markup, &match_result)?;
            if let Ok(func) = self.lua.registry_value::<Function>(&handler.callback_key)
                && let Err(err) = func.call_async::<()>(event).await
            {
                println!("  \x1b[31m[HANDLER ERROR]\x1b[0m {err}");
            }
        }

        Ok(())
    }

    /// Checks if a Lua value is the simulated event table.
    fn is_sim_event_table(val: &Value) -> bool {
        if let Value::Table(t) = val {
            t.contains_key("incoming").unwrap_or(false)
                && t.contains_key("chat_id").unwrap_or(false)
        } else {
            false
        }
    }

    /// Shifts arguments if called with method colon syntax `event:method(...)`.
    fn shift_sim_event_args(
        a1: Value,
        a2: Option<Value>,
        a3: Option<Value>,
    ) -> (Value, Option<Value>, Option<Value>) {
        if Self::is_sim_event_table(&a1) {
            (a2.unwrap_or(Value::Nil), a3, None)
        } else {
            (a1, a2, a3)
        }
    }

    /// Converts an optional Lua value to f64 delay.
    fn value_to_f64(val: Option<Value>) -> Option<f64> {
        match val {
            Some(Value::Number(n)) => Some(n),
            Some(Value::Integer(i)) => Some(i as f64),
            _ => None,
        }
    }

    /// Creates a simulated `event` Lua table matching production runner semantics.
    fn create_sim_event(
        &self,
        text: &str,
        chat_id: i64,
        markup: &MessageMarkup,
        match_result: &FilterMatchResult,
    ) -> Result<Table, OxideError> {
        let event = self.lua.create_table().map_err(ScriptError::LuaError)?;
        let msg_id = rand::random::<u16>() as i32;

        event.set("id", msg_id).map_err(ScriptError::LuaError)?;
        event
            .set("message_id", msg_id)
            .map_err(ScriptError::LuaError)?;
        event.set("text", text).map_err(ScriptError::LuaError)?;
        event
            .set("chat_id", chat_id)
            .map_err(ScriptError::LuaError)?;
        event
            .set("sender_id", chat_id)
            .map_err(ScriptError::LuaError)?;
        event.set("incoming", true).map_err(ScriptError::LuaError)?;
        event
            .set("outgoing", false)
            .map_err(ScriptError::LuaError)?;
        event
            .set("is_private", true)
            .map_err(ScriptError::LuaError)?;
        event
            .set("is_group", false)
            .map_err(ScriptError::LuaError)?;
        event
            .set("is_channel", false)
            .map_err(ScriptError::LuaError)?;

        // Regex matches & captures
        let matches_tbl = self.lua.create_table().map_err(ScriptError::LuaError)?;
        for (i, m) in match_result.matches.iter().enumerate() {
            matches_tbl
                .raw_set(i + 1, m.clone())
                .map_err(ScriptError::LuaError)?;
        }
        for (k, v) in &match_result.captures {
            matches_tbl
                .raw_set(k.as_str(), v.clone())
                .map_err(ScriptError::LuaError)?;
        }
        event
            .set("matches", matches_tbl.clone())
            .map_err(ScriptError::LuaError)?;
        event
            .set("captures", matches_tbl)
            .map_err(ScriptError::LuaError)?;

        // Buttons
        let buttons = markup_to_lua_table(&self.lua, markup).map_err(ScriptError::LuaError)?;
        event
            .set("buttons", buttons)
            .map_err(ScriptError::LuaError)?;

        // event.reply
        let reply_fn = self
            .lua
            .create_function(|_, (a1, a2, a3): (Value, Option<Value>, Option<Value>)| {
                let (arg1, delay_val, _) = Self::shift_sim_event_args(a1, a2, a3);
                let (reply_text, delay) = if let Value::Table(ref t) = arg1 {
                    let text: String = t
                        .get("text")
                        .or_else(|_| t.get("message"))
                        .map_err(|e| mlua::Error::RuntimeError(e.to_string()))?;
                    let d: Option<f64> = t.get("delay").ok();
                    (text, d)
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
                    (text, Self::value_to_f64(delay_val))
                };
                let d_str = delay
                    .map(|d| format!(" (delay: {d:.1}s)"))
                    .unwrap_or_default();
                println!("  \x1b[32m[EVENT REPLY]\x1b[0m \"{reply_text}\"{d_str}");
                Ok(())
            })
            .map_err(ScriptError::LuaError)?;
        event
            .set("reply", reply_fn)
            .map_err(ScriptError::LuaError)?;

        // event.click
        let markup_copy = markup.clone();
        let click_fn = self
            .lua
            .create_async_function(
                move |_, (a1, a2, a3): (Value, Option<Value>, Option<Value>)| {
                    let markup = markup_copy.clone();
                    async move {
                        let (arg1, delay_val, _) = Self::shift_sim_event_args(a1, a2, a3);
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

                        let d_str = delay
                            .map(|d| format!(" (delay: {d:.1}s)"))
                            .unwrap_or_default();

                        let btn = match &query_val {
                            Value::Integer(i) if *i >= 1 => {
                                markup.flat_button_at((*i - 1) as usize)
                            }
                            Value::Number(n) if *n >= 1.0 => {
                                markup.flat_button_at((*n as usize) - 1)
                            }
                            Value::String(s) => {
                                let q = s
                                    .to_str()
                                    .map_err(|e| mlua::Error::RuntimeError(e.to_string()))?;
                                markup.find_button(&q).map(|(_, _, b)| b)
                            }
                            _ => None,
                        };

                        if let Some(button) = btn {
                            println!(
                                "  \x1b[33m[EVENT CLICK]\x1b[0m Matched button: \"{}\"{d_str}",
                                button.text
                            );
                            Ok(button.text.clone())
                        } else if let Value::String(s) = query_val {
                            let query_str = s
                                .to_str()
                                .map_err(|e| mlua::Error::RuntimeError(e.to_string()))?
                                .to_string();
                            println!("  \x1b[33m[EVENT CLICK FALLBACK]\x1b[0m Sent button text: \"{query_str}\"{d_str}");
                            Ok(query_str)
                        } else {
                            Err(mlua::Error::RuntimeError(
                                "Button not found on message".into(),
                            ))
                        }
                    }
                },
            )
            .map_err(ScriptError::LuaError)?;
        event
            .set("click", click_fn)
            .map_err(ScriptError::LuaError)?;

        // event.edit
        let edit_fn = self
            .lua
            .create_function(|_, (a1, a2, a3): (Value, Option<Value>, Option<Value>)| {
                let (arg1, delay_val, _) = Self::shift_sim_event_args(a1, a2, a3);
                let (new_text, delay) = if let Value::Table(ref t) = arg1 {
                    let text: String = t
                        .get("text")
                        .or_else(|_| t.get("message"))
                        .map_err(|e| mlua::Error::RuntimeError(e.to_string()))?;
                    let d: Option<f64> = t.get("delay").ok();
                    (text, d)
                } else {
                    let text = match arg1 {
                        Value::String(s) => s.to_str()?.to_string(),
                        Value::Integer(i) => i.to_string(),
                        Value::Number(n) => n.to_string(),
                        _ => {
                            return Err(mlua::Error::RuntimeError(
                                "edit expects new text as first argument or table".into(),
                            ));
                        }
                    };
                    (text, Self::value_to_f64(delay_val))
                };
                let d_str = delay
                    .map(|d| format!(" (delay: {d:.1}s)"))
                    .unwrap_or_default();
                println!("  \x1b[36m[EVENT EDIT]\x1b[0m \"{new_text}\"{d_str}");
                Ok(())
            })
            .map_err(ScriptError::LuaError)?;
        event.set("edit", edit_fn).map_err(ScriptError::LuaError)?;

        // event.delete
        let del_fn = self
            .lua
            .create_function(|_, (a1, a2): (Option<Value>, Option<Value>)| {
                let delay = if a1.as_ref().map(Self::is_sim_event_table).unwrap_or(false) {
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
                let d_str = delay
                    .map(|d| format!(" (delay: {d:.1}s)"))
                    .unwrap_or_default();
                println!("  \x1b[31m[EVENT DELETE]\x1b[0m Message deleted{d_str}");
                Ok(())
            })
            .map_err(ScriptError::LuaError)?;
        event.set("delete", del_fn).map_err(ScriptError::LuaError)?;

        // event.react
        let react_fn = self
            .lua
            .create_function(|_, (a1, a2, a3): (Value, Option<Value>, Option<Value>)| {
                let (arg1, delay_val, _) = Self::shift_sim_event_args(a1, a2, a3);
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
                let d_str = delay
                    .map(|d| format!(" (delay: {d:.1}s)"))
                    .unwrap_or_default();
                println!("  \x1b[35m[EVENT REACT]\x1b[0m Emoji: {emoji}{d_str}");
                Ok(())
            })
            .map_err(ScriptError::LuaError)?;
        event
            .set("react", react_fn)
            .map_err(ScriptError::LuaError)?;

        // event.pin
        let pin_fn = self
            .lua
            .create_function(|_, (a1, a2): (Option<Value>, Option<Value>)| {
                let delay = if a1.as_ref().map(Self::is_sim_event_table).unwrap_or(false) {
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
                let d_str = delay
                    .map(|d| format!(" (delay: {d:.1}s)"))
                    .unwrap_or_default();
                println!("  \x1b[35m[EVENT PIN]\x1b[0m Message pinned{d_str}");
                Ok(())
            })
            .map_err(ScriptError::LuaError)?;
        event.set("pin", pin_fn).map_err(ScriptError::LuaError)?;

        Ok(event)
    }

    /// Runs the interactive terminal REPL loop.
    pub async fn run_loop(&mut self) -> Result<(), OxideError> {
        let bot_name = self
            .script_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();

        println!(
            "\n\x1b[1;36m========================================================================\x1b[0m"
        );
        println!("\x1b[1;36m  OXIDEGRAM DRY-RUN SIMULATOR (Offline Mock Environment)\x1b[0m");
        println!(
            "  Bot Script:  \x1b[33m{}\x1b[0m (Hot-Reload enabled)",
            self.script_path.display()
        );
        println!(
            "  Target Chat: \x1b[32m{}\x1b[0m",
            *self.active_chat_id.read().await
        );
        println!("\x1b[90m  Type message text to simulate incoming message from user/bot.");
        println!("  Commands: /click <text>, /buttons <b1,b2>, /chat <id>, /state, /exit\x1b[0m");
        println!(
            "\x1b[1;36m========================================================================\x1b[0m\n"
        );

        // Set up file watcher for Hot-Reload
        let (fs_tx, mut fs_rx) = tokio::sync::mpsc::channel(10);
        let mut watcher = notify::recommended_watcher(move |res| {
            let _ = fs_tx.blocking_send(res);
        })
        .map_err(|e| ScriptError::Runtime(format!("Watcher error: {e}")))?;

        if let Some(parent) = self.script_path.parent() {
            let _ = watcher.watch(parent, RecursiveMode::NonRecursive);
        }

        let mut reader = tokio::io::BufReader::new(tokio::io::stdin());
        use tokio::io::AsyncBufReadExt;

        loop {
            if *self.stop_rx.borrow() {
                println!("\n[SIMULATOR] Bot event loop stopped.");
                break;
            }

            tokio::select! {
                Some(fs_res) = fs_rx.recv() => {
                    if let Ok(ev) = fs_res && matches!(ev.kind, EventKind::Modify(_) | EventKind::Create(_)) {
                        let target_name = self.script_path.file_name();
                        if ev.paths.iter().any(|p| p.file_name() == target_name) {
                            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                            while fs_rx.try_recv().is_ok() {}
                            println!("\n\x1b[33m[SIMULATOR HOT-RELOAD]\x1b[0m Reloading script...");
                            self.handlers.lock().await.clear();
                            self.lua = Lua::new();
                            if let Err(e) = self.load_script().await {
                                println!("\x1b[31m[RELOAD ERROR]\x1b[0m {e}");
                            } else {
                                println!("\x1b[32m[RELOAD OK]\x1b[0m Script reloaded successfully.");
                            }
                        }
                    }
                }
                res = async {
                    use std::io::Write;
                    print!("\x1b[1;34m[Mock Chat]\x1b[0m > ");
                    let _ = std::io::stdout().flush();
                    let mut line = String::new();
                    reader.read_line(&mut line).await.map(|_| line)
                } => {
                    let line = match res {
                        Ok(l) => l.trim().to_string(),
                        Err(_) => break,
                    };

                    if line.is_empty() {
                        continue;
                    }

                    if line == "/exit" || line == "/quit" {
                        println!("[SIMULATOR] Exiting simulator.");
                        break;
                    } else if line == "/help" {
                        println!("Commands:");
                        println!("  /click <text>            Simulate clicking a button");
                        println!("  /buttons <btn1>, <btn2>  Attach buttons to subsequent incoming messages");
                        println!("  /chat <id>               Change active chat ID");
                        println!("  /state                   View persistent ox.storage data");
                        println!("  /exit, /quit             Exit simulator");
                        continue;
                    } else if let Some(rest) = line.strip_prefix("/chat ") {
                        if let Ok(id) = rest.trim().parse::<i64>() {
                            *self.active_chat_id.write().await = id;
                            println!("Active chat ID changed to {id}");
                        } else {
                            println!("Invalid chat ID");
                        }
                        continue;
                    } else if let Some(rest) = line.strip_prefix("/buttons ") {
                        let btns: Vec<BotButton> = rest
                            .split(',')
                            .map(|b| BotButton::new(b.trim(), ButtonKind::Text))
                            .collect();
                        let count = btns.len();
                        *self.active_markup.write().await = MessageMarkup {
                            rows: vec![KeyboardRow { buttons: btns }],
                            is_inline: false,
                        };
                        println!("Active buttons set ({count} buttons). Next messages will include them.");
                        continue;
                    } else if line == "/state" {
                        let all = self.storage.get_all().await;
                        println!("Current ox.storage ({bot_name}):");
                        for (k, v) in all.iter() {
                            println!("  {k} = {v}");
                        }
                        continue;
                    } else if let Some(rest) = line.strip_prefix("/click ") {
                        let btn_text = rest.trim();
                        println!("Simulating user click: \"{btn_text}\"");
                        self.simulate_incoming(btn_text).await?;
                        continue;
                    }

                    // Regular simulated message
                    self.simulate_incoming(&line).await?;
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockConsole;
    impl BotConsole for MockConsole {
        fn ask_input(&self, _prompt: &str, default: Option<&str>) -> Result<String, String> {
            Ok(default.unwrap_or("default_val").to_string())
        }

        fn ask_select(
            &self,
            _prompt: &str,
            choices: Vec<String>,
            default: Option<&str>,
        ) -> Result<String, String> {
            Ok(default.unwrap_or(&choices[0]).to_string())
        }

        fn ask_confirm(&self, _prompt: &str, default: Option<bool>) -> Result<bool, String> {
            Ok(default.unwrap_or(true))
        }

        fn ask_file(
            &self,
            _prompt: &str,
            default: Option<&str>,
            _must_exist: bool,
            _allowed_extensions: Option<Vec<String>>,
        ) -> Result<String, String> {
            Ok(default.unwrap_or("test.txt").to_string())
        }
    }

    #[tokio::test]
    async fn test_simulator_ox_select_and_confirm() {
        let temp_dir = std::env::temp_dir();
        let script_path = temp_dir.join("test_sim_select.lua");
        std::fs::write(&script_path, "ox.on_message(function(e) end)").unwrap();

        let console = Arc::new(MockConsole);
        let mut sim = BotSimulator::new(&script_path, console).await.unwrap();
        sim.load_script().await.unwrap();

        // 1. ox.select with string array
        let res: String = sim
            .lua
            .load(r#"return ox.select("Mode?", { "text", "grades" }, "grades")"#)
            .eval()
            .unwrap();
        assert_eq!(res, "grades");

        // 1b. ox.select with named table syntax
        let res_named: String = sim
            .lua
            .load(
                r#"return ox.select { prompt = "Working mode?", options = { "text", "grade" }, default = "text" }"#,
            )
            .eval()
            .unwrap();
        assert_eq!(res_named, "text");

        // 2. ox.select with table items { label, value }
        let res: String = sim
            .lua
            .load(
                r#"return ox.select("Mode?", { { label = "Text Msg", value = "txt" }, { label = "Grade Spam", value = "grd" } }, "txt")"#,
            )
            .eval()
            .unwrap();
        assert_eq!(res, "txt");

        // 3. ox.confirm
        let res: bool = sim
            .lua
            .load(r#"return ox.confirm("Delete?", false)"#)
            .eval()
            .unwrap();
        assert!(!res);

        // 3b. ox.confirm with named table
        let res_named_conf: bool = sim
            .lua
            .load(r#"return ox.confirm { prompt = "Delete?", default = false }"#)
            .eval()
            .unwrap();
        assert!(!res_named_conf);

        // 4. ox.input with default string and table
        let res: String = sim
            .lua
            .load(r#"return ox.input("Mode?", "text")"#)
            .eval()
            .unwrap();
        assert_eq!(res, "text");

        let res_tbl: String = sim
            .lua
            .load(r#"return ox.input { prompt = "Mode?", default = "text_named" }"#)
            .eval()
            .unwrap();
        assert_eq!(res_tbl, "text_named");

        // 5. ox.file
        let file_res: String = sim
            .lua
            .load(r#"return ox.file("Path?", { default = "config.json", must_exist = false })"#)
            .eval()
            .unwrap();
        assert_eq!(file_res, "config.json");

        // 6. ox.log dot and colon syntax
        sim.lua
            .load(r#"ox.log.info("dot log"); ox.log:info("colon log")"#)
            .exec()
            .unwrap();
    }

    #[tokio::test]
    async fn test_simulator_event_colon_syntax() {
        let temp_dir = std::env::temp_dir();
        let script_path = temp_dir.join("test_sim_colon.lua");
        let script = r#"
            local handled = false
            ox.on_message(function(event)
                -- Colon method invocation
                event:reply("hello")
                event:edit("new text")
                event:react("👍")
                event:pin()
                event:delete()

                -- Named table syntax
                event:reply { text = "table reply", delay = 0.01 }
                event:edit { text = "table edit" }
                event:react { emoji = "🔥" }
                event:pin { delay = 0.01 }
                event:delete { delay = 0.01 }

                -- Standalone ox functions with named table syntax
                ox.send_message { chat_id = 12345, text = "hi named" }
                ox.send_image { chat_id = 12345, path = "pic.png", caption = "photo" }

                handled = true
            end)
            return true
        "#;
        std::fs::write(&script_path, script).unwrap();

        let console = Arc::new(MockConsole);
        let mut sim = BotSimulator::new(&script_path, console).await.unwrap();
        sim.load_script().await.unwrap();

        // Simulate incoming message
        sim.simulate_incoming("test message").await.unwrap();
    }
}
