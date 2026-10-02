//! Registration and bindings for global `ox.*` Lua API methods.

use crate::application::automation::BotConsole;
use crate::domain::automation::MessageFilter;
use crate::errors::ScriptError;
use crate::infrastructure::NETWORK_TIMEOUT;
use crate::infrastructure::lua::filters::parse_message_filter;
use grammers_client::Client;
use grammers_client::message::InputMessage;
use grammers_session::types::PeerRef;
use grammers_tl_types as tl;
use mlua::{Function, Lua, RegistryKey, Table, Value};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock, watch};

/// Registered message handler containing callback key and optional filter.
pub struct RegisteredHandler {
    pub callback_key: RegistryKey,
    pub filter: Option<MessageFilter>,
}

/// Manager tracking active background interval and timeout timers.
pub struct TimerHub {
    next_id: std::sync::atomic::AtomicU64,
    cancel_txs: Mutex<HashMap<u64, tokio::sync::oneshot::Sender<()>>>,
    event_tx: tokio::sync::mpsc::Sender<Arc<RegistryKey>>,
}

impl TimerHub {
    pub fn new(event_tx: tokio::sync::mpsc::Sender<Arc<RegistryKey>>) -> Self {
        Self {
            next_id: std::sync::atomic::AtomicU64::new(1),
            cancel_txs: Mutex::new(HashMap::new()),
            event_tx,
        }
    }

    /// Spawns a background timer and schedules callback execution via the runner queue.
    pub async fn add_timer(
        &self,
        key: RegistryKey,
        duration: std::time::Duration,
        is_interval: bool,
    ) -> u64 {
        let id = self
            .next_id
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let (cancel_tx, mut cancel_rx) = tokio::sync::oneshot::channel();
        self.cancel_txs.lock().await.insert(id, cancel_tx);

        let event_tx = self.event_tx.clone();
        let key_arc = Arc::new(key);
        tokio::spawn(async move {
            if is_interval {
                let mut interval = tokio::time::interval(duration);
                interval.tick().await; // Initial tick
                loop {
                    tokio::select! {
                        _ = &mut cancel_rx => break,
                        _ = interval.tick() => {
                            if event_tx.send(Arc::clone(&key_arc)).await.is_err() {
                                break;
                            }
                        }
                    }
                }
            } else {
                tokio::select! {
                    _ = &mut cancel_rx => {},
                    _ = tokio::time::sleep(duration) => {
                        let _ = event_tx.send(key_arc).await;
                    }
                }
            }
        });

        id
    }

    /// Cancels an active timer by identifier.
    pub async fn clear_timer(&self, id: u64) -> bool {
        if let Some(tx) = self.cancel_txs.lock().await.remove(&id) {
            let _ = tx.send(());
            true
        } else {
            false
        }
    }

    /// Cancels all active timers.
    pub async fn cancel_all(&self) {
        let mut map = self.cancel_txs.lock().await;
        for (_, tx) in map.drain() {
            let _ = tx.send(());
        }
    }
}

/// Helper function to asynchronously sleep for the given delay with subtle jitter (to humanize requests).
pub async fn apply_optional_delay(delay: Option<f64>) {
    if let Some(d) = delay
        && d > 0.0
    {
        let jitter_factor = (rand::random::<f64>() - 0.5) * 0.2;
        let final_delay = (d + d * jitter_factor).max(0.01);
        tokio::time::sleep(std::time::Duration::from_secs_f64(final_delay)).await;
    }
}

/// Helper function to build an `InputMessage` applying Markdown or HTML parsing if configured.
pub fn build_input_message(text: &str, parse_mode: Option<&str>) -> InputMessage {
    match parse_mode.map(|s| s.to_ascii_lowercase()).as_deref() {
        Some("markdown") | Some("md") => InputMessage::new().markdown(text),
        Some("html") => InputMessage::new().html(text),
        _ => InputMessage::new().text(text),
    }
}

/// Options parsed from an optional Lua table argument.
pub struct MessageOptions {
    pub delay: Option<f64>,
    pub parse_mode: Option<String>,
}

/// Parses message options from either a number (delay) or a table ({ delay = 1.0, parse_mode = "markdown" }).
pub fn parse_message_options(val: Option<Value>) -> Result<MessageOptions, mlua::Error> {
    match val {
        Some(Value::Number(n)) => Ok(MessageOptions {
            delay: Some(n),
            parse_mode: None,
        }),
        Some(Value::Integer(i)) => Ok(MessageOptions {
            delay: Some(i as f64),
            parse_mode: None,
        }),
        Some(Value::Table(t)) => {
            let delay: Option<f64> = t.get("delay").ok();
            let parse_mode: Option<String> = t.get("parse_mode").ok();
            Ok(MessageOptions { delay, parse_mode })
        }
        _ => Ok(MessageOptions {
            delay: None,
            parse_mode: None,
        }),
    }
}

/// Parses `send_message` / `reply` arguments supporting both positional and single-table named argument forms.
pub fn parse_send_message_args(
    arg1: Value,
    arg2: Option<Value>,
    arg3: Option<Value>,
) -> Result<(i64, String, MessageOptions), mlua::Error> {
    if let Value::Table(tbl) = &arg1
        && (tbl.contains_key("text")? || tbl.contains_key("message")?)
    {
        let chat_id = tbl
            .get::<i64>("chat_id")
            .or_else(|_| tbl.get::<i64>("chat"))
            .or_else(|_| tbl.get::<i64>("to"))
            .map_err(|_| {
                mlua::Error::RuntimeError(
                    "send_message named table argument requires 'chat_id'".into(),
                )
            })?;
        let text = tbl
            .get::<String>("text")
            .or_else(|_| tbl.get::<String>("message"))
            .map_err(|_| {
                mlua::Error::RuntimeError(
                    "send_message named table argument requires 'text'".into(),
                )
            })?;
        let delay: Option<f64> = tbl.get("delay").ok();
        let parse_mode: Option<String> = tbl.get("parse_mode").ok();
        return Ok((chat_id, text, MessageOptions { delay, parse_mode }));
    }

    let chat_id = match arg1 {
        Value::Integer(i) => i,
        Value::Number(n) => n as i64,
        _ => {
            return Err(mlua::Error::RuntimeError(
                "send_message expects chat_id (number) or table with named parameters".into(),
            ));
        }
    };
    let text = match arg2 {
        Some(Value::String(s)) => s.to_str()?.to_string(),
        Some(Value::Integer(i)) => i.to_string(),
        Some(Value::Number(n)) => n.to_string(),
        _ => {
            return Err(mlua::Error::RuntimeError(
                "send_message expects text (string) as second parameter".into(),
            ));
        }
    };
    let opts = parse_message_options(arg3)?;
    Ok((chat_id, text, opts))
}

/// Parses `edit_message` arguments supporting both positional and single-table named argument forms.
pub fn parse_edit_message_args(
    arg1: Value,
    arg2: Option<Value>,
    arg3: Option<Value>,
    arg4: Option<Value>,
) -> Result<(i64, i32, String, MessageOptions), mlua::Error> {
    if let Value::Table(t) = &arg1
        && (t.contains_key("text")? || t.contains_key("message")?)
    {
        let chat_id = t
            .get::<i64>("chat_id")
            .or_else(|_| t.get::<i64>("chat"))
            .map_err(|_| {
                mlua::Error::RuntimeError(
                    "edit_message named table argument requires 'chat_id'".into(),
                )
            })?;
        let message_id = t
            .get::<i32>("message_id")
            .or_else(|_| t.get::<i32>("id"))
            .map_err(|_| {
                mlua::Error::RuntimeError(
                    "edit_message named table argument requires 'message_id'".into(),
                )
            })?;
        let text = t
            .get::<String>("text")
            .or_else(|_| t.get::<String>("message"))
            .map_err(|_| {
                mlua::Error::RuntimeError(
                    "edit_message named table argument requires 'text'".into(),
                )
            })?;
        let delay: Option<f64> = t.get("delay").ok();
        let parse_mode: Option<String> = t.get("parse_mode").ok();
        return Ok((
            chat_id,
            message_id,
            text,
            MessageOptions { delay, parse_mode },
        ));
    }

    let chat_id = match arg1 {
        Value::Integer(i) => i,
        Value::Number(n) => n as i64,
        _ => {
            return Err(mlua::Error::RuntimeError(
                "edit_message expects chat_id (number) or table".into(),
            ));
        }
    };
    let message_id = match arg2 {
        Some(Value::Integer(i)) => i as i32,
        Some(Value::Number(n)) => n as i32,
        _ => {
            return Err(mlua::Error::RuntimeError(
                "edit_message expects message_id (number) as second parameter".into(),
            ));
        }
    };
    let text = match arg3 {
        Some(Value::String(s)) => s.to_str()?.to_string(),
        Some(Value::Integer(i)) => i.to_string(),
        Some(Value::Number(n)) => n.to_string(),
        _ => {
            return Err(mlua::Error::RuntimeError(
                "edit_message expects new text as third parameter".into(),
            ));
        }
    };
    let opts = parse_message_options(arg4)?;
    Ok((chat_id, message_id, text, opts))
}

/// Parses `delete_message` arguments supporting both positional and single-table named argument forms.
pub fn parse_delete_message_args(
    arg1: Value,
    arg2: Option<Value>,
    arg3: Option<Value>,
) -> Result<(i64, Vec<i32>, Option<f64>), mlua::Error> {
    if let Value::Table(t) = &arg1
        && (t.contains_key("chat_id")? || t.contains_key("chat")?)
    {
        let chat_id = t.get::<i64>("chat_id").or_else(|_| t.get::<i64>("chat"))?;
        let delay: Option<f64> = t.get("delay").ok();
        let ids = if let Ok(id) = t.get::<i32>("message_id") {
            vec![id]
        } else if let Ok(id) = t.get::<i32>("id") {
            vec![id]
        } else if let Ok(ids_tbl) = t.get::<Table>("ids") {
            let mut list = Vec::new();
            for id in ids_tbl.sequence_values::<i32>().flatten() {
                list.push(id);
            }
            list
        } else {
            return Err(mlua::Error::RuntimeError(
                "delete_message requires 'message_id' or 'ids'".into(),
            ));
        };
        return Ok((chat_id, ids, delay));
    }

    let chat_id = match arg1 {
        Value::Integer(i) => i,
        Value::Number(n) => n as i64,
        _ => {
            return Err(mlua::Error::RuntimeError(
                "delete_message expects chat_id (number) or table".into(),
            ));
        }
    };

    let ids = match arg2 {
        Some(Value::Integer(i)) => vec![i as i32],
        Some(Value::Number(n)) => vec![n as i32],
        Some(Value::Table(t)) => {
            let mut list = Vec::new();
            for id in t.sequence_values::<i32>().flatten() {
                list.push(id);
            }
            list
        }
        _ => {
            return Err(mlua::Error::RuntimeError(
                "delete_message expects message_id or array of IDs".into(),
            ));
        }
    };

    let delay = match arg3 {
        Some(Value::Number(n)) => Some(n),
        Some(Value::Integer(i)) => Some(i as f64),
        _ => None,
    };

    Ok((chat_id, ids, delay))
}

/// Parses `send_reaction` arguments supporting both positional and single-table named argument forms.
pub fn parse_react_args(
    arg1: Value,
    arg2: Option<Value>,
    arg3: Option<Value>,
    arg4: Option<Value>,
) -> Result<(i64, i32, String, Option<f64>), mlua::Error> {
    if let Value::Table(t) = &arg1
        && (t.contains_key("reaction")? || t.contains_key("emoji")?)
    {
        let chat_id = t
            .get::<i64>("chat_id")
            .or_else(|_| t.get::<i64>("chat"))
            .map_err(|_| {
                mlua::Error::RuntimeError("send_reaction named table requires 'chat_id'".into())
            })?;
        let message_id = t
            .get::<i32>("message_id")
            .or_else(|_| t.get::<i32>("id"))
            .map_err(|_| {
                mlua::Error::RuntimeError("send_reaction named table requires 'message_id'".into())
            })?;
        let reaction = t
            .get::<String>("reaction")
            .or_else(|_| t.get::<String>("emoji"))
            .map_err(|_| {
                mlua::Error::RuntimeError(
                    "send_reaction named table requires 'reaction' or 'emoji'".into(),
                )
            })?;
        let delay: Option<f64> = t.get("delay").ok();
        return Ok((chat_id, message_id, reaction, delay));
    }

    let chat_id = match arg1 {
        Value::Integer(i) => i,
        Value::Number(n) => n as i64,
        _ => {
            return Err(mlua::Error::RuntimeError(
                "send_reaction expects chat_id (number) or table".into(),
            ));
        }
    };
    let message_id = match arg2 {
        Some(Value::Integer(i)) => i as i32,
        Some(Value::Number(n)) => n as i32,
        _ => {
            return Err(mlua::Error::RuntimeError(
                "send_reaction expects message_id as second argument".into(),
            ));
        }
    };
    let emoji = match arg3 {
        Some(Value::String(s)) => s.to_str()?.to_string(),
        _ => {
            return Err(mlua::Error::RuntimeError(
                "send_reaction expects emoji string as third argument".into(),
            ));
        }
    };
    let delay = match arg4 {
        Some(Value::Number(n)) => Some(n),
        Some(Value::Integer(i)) => Some(i as f64),
        _ => None,
    };
    Ok((chat_id, message_id, emoji, delay))
}

/// Parses `pin_message` arguments supporting both positional and single-table named argument forms.
pub fn parse_pin_args(
    arg1: Value,
    arg2: Option<Value>,
    arg3: Option<Value>,
) -> Result<(i64, i32, Option<f64>), mlua::Error> {
    if let Value::Table(t) = &arg1
        && (t.contains_key("message_id")? || t.contains_key("id")?)
    {
        let chat_id = t
            .get::<i64>("chat_id")
            .or_else(|_| t.get::<i64>("chat"))
            .map_err(|_| {
                mlua::Error::RuntimeError("pin_message table requires 'chat_id'".into())
            })?;
        let message_id = t
            .get::<i32>("message_id")
            .or_else(|_| t.get::<i32>("id"))
            .map_err(|_| {
                mlua::Error::RuntimeError("pin_message table requires 'message_id'".into())
            })?;
        let delay: Option<f64> = t.get("delay").ok();
        return Ok((chat_id, message_id, delay));
    }

    let chat_id = match arg1 {
        Value::Integer(i) => i,
        Value::Number(n) => n as i64,
        _ => {
            return Err(mlua::Error::RuntimeError(
                "pin_message expects chat_id (number) or table".into(),
            ));
        }
    };
    let message_id = match arg2 {
        Some(Value::Integer(i)) => i as i32,
        Some(Value::Number(n)) => n as i32,
        _ => {
            return Err(mlua::Error::RuntimeError(
                "pin_message expects message_id as second parameter".into(),
            ));
        }
    };
    let delay = match arg3 {
        Some(Value::Number(n)) => Some(n),
        Some(Value::Integer(i)) => Some(i as f64),
        _ => None,
    };

    Ok((chat_id, message_id, delay))
}

/// Parses `forward_message` arguments supporting both positional and single-table named argument forms.
pub fn parse_forward_args(
    arg1: Value,
    arg2: Option<Value>,
    arg3: Option<Value>,
) -> Result<(i64, i64, i32), mlua::Error> {
    if let Value::Table(t) = &arg1
        && (t.contains_key("message_id")? || t.contains_key("id")?)
    {
        let to_chat = t
            .get::<i64>("to_chat_id")
            .or_else(|_| t.get::<i64>("to"))
            .map_err(|_| {
                mlua::Error::RuntimeError(
                    "forward_message table requires 'to_chat_id' or 'to'".into(),
                )
            })?;
        let from_chat = t
            .get::<i64>("from_chat_id")
            .or_else(|_| t.get::<i64>("from"))
            .map_err(|_| {
                mlua::Error::RuntimeError(
                    "forward_message table requires 'from_chat_id' or 'from'".into(),
                )
            })?;
        let message_id = t
            .get::<i32>("message_id")
            .or_else(|_| t.get::<i32>("id"))
            .map_err(|_| {
                mlua::Error::RuntimeError(
                    "forward_message table requires 'message_id' or 'id'".into(),
                )
            })?;
        return Ok((to_chat, from_chat, message_id));
    }

    let to_chat = match arg1 {
        Value::Integer(i) => i,
        Value::Number(n) => n as i64,
        _ => {
            return Err(mlua::Error::RuntimeError(
                "forward_message expects to_chat_id (number) or table".into(),
            ));
        }
    };
    let from_chat = match arg2 {
        Some(Value::Integer(i)) => i,
        Some(Value::Number(n)) => n as i64,
        _ => {
            return Err(mlua::Error::RuntimeError(
                "forward_message expects from_chat_id (number) as second parameter".into(),
            ));
        }
    };
    let message_id = match arg3 {
        Some(Value::Integer(i)) => i as i32,
        Some(Value::Number(n)) => n as i32,
        _ => {
            return Err(mlua::Error::RuntimeError(
                "forward_message expects message_id as third parameter".into(),
            ));
        }
    };

    Ok((to_chat, from_chat, message_id))
}

/// Parses media sending arguments (`send_image`, `send_document`, `send_audio`, `send_voice`).
pub fn parse_media_send_args(
    fn_name: &str,
    arg1: Value,
    arg2: Option<Value>,
    arg3: Option<Value>,
    arg4: Option<Value>,
) -> Result<(i64, String, Option<String>, MessageOptions), mlua::Error> {
    if let Value::Table(t) = &arg1
        && (t.contains_key("path")? || t.contains_key("file")?)
    {
        let chat_id = t
            .get::<i64>("chat_id")
            .or_else(|_| t.get::<i64>("chat"))
            .or_else(|_| t.get::<i64>("to"))
            .map_err(|_| {
                mlua::Error::RuntimeError(format!("{fn_name} table argument requires 'chat_id'"))
            })?;
        let path = t
            .get::<String>("path")
            .or_else(|_| t.get::<String>("file"))
            .map_err(|_| {
                mlua::Error::RuntimeError(format!("{fn_name} table argument requires 'path'"))
            })?;
        let caption: Option<String> = t.get("caption").ok();
        let delay: Option<f64> = t.get("delay").ok();
        let parse_mode: Option<String> = t.get("parse_mode").ok();
        return Ok((chat_id, path, caption, MessageOptions { delay, parse_mode }));
    }

    let chat_id = match arg1 {
        Value::Integer(i) => i,
        Value::Number(n) => n as i64,
        _ => {
            return Err(mlua::Error::RuntimeError(format!(
                "{fn_name} expects chat_id (number) or table with named parameters"
            )));
        }
    };

    let path = match arg2 {
        Some(Value::String(s)) => s.to_str()?.to_string(),
        _ => {
            return Err(mlua::Error::RuntimeError(format!(
                "{fn_name} expects path (string) as second parameter"
            )));
        }
    };

    let (caption, opts) = match (arg3, arg4) {
        (Some(Value::String(c)), fourth) => {
            let opts = parse_message_options(fourth)?;
            (Some(c.to_str()?.to_string()), opts)
        }
        (Some(Value::Table(t)), _) => {
            let opts = parse_message_options(Some(Value::Table(t)))?;
            (None, opts)
        }
        (Some(Value::Number(n)), _) => {
            let opts = parse_message_options(Some(Value::Number(n)))?;
            (None, opts)
        }
        (Some(Value::Integer(i)), _) => {
            let opts = parse_message_options(Some(Value::Integer(i)))?;
            (None, opts)
        }
        _ => (None, parse_message_options(None)?),
    };

    Ok((chat_id, path, caption, opts))
}

/// Parses `click_button` arguments.
pub fn parse_click_button_args(
    arg1: Value,
    arg2: Option<Value>,
    arg3: Option<Value>,
) -> Result<(i64, i32, String), mlua::Error> {
    if let Value::Table(t) = &arg1
        && (t.contains_key("data")? || t.contains_key("query")?)
    {
        let chat_id = t
            .get::<i64>("chat_id")
            .or_else(|_| t.get::<i64>("chat"))
            .map_err(|_| {
                mlua::Error::RuntimeError("click_button table requires 'chat_id'".into())
            })?;
        let message_id = t
            .get::<i32>("message_id")
            .or_else(|_| t.get::<i32>("id"))
            .map_err(|_| {
                mlua::Error::RuntimeError("click_button table requires 'message_id'".into())
            })?;
        let data = t
            .get::<String>("data")
            .or_else(|_| t.get::<String>("query"))
            .map_err(|_| mlua::Error::RuntimeError("click_button table requires 'data'".into()))?;
        return Ok((chat_id, message_id, data));
    }

    let chat_id = match arg1 {
        Value::Integer(i) => i,
        Value::Number(n) => n as i64,
        _ => {
            return Err(mlua::Error::RuntimeError(
                "click_button expects chat_id as first argument".into(),
            ));
        }
    };
    let message_id = match arg2 {
        Some(Value::Integer(i)) => i as i32,
        Some(Value::Number(n)) => n as i32,
        _ => {
            return Err(mlua::Error::RuntimeError(
                "click_button expects message_id as second argument".into(),
            ));
        }
    };
    let data = match arg3 {
        Some(Value::String(s)) => s.to_str()?.to_string(),
        _ => {
            return Err(mlua::Error::RuntimeError(
                "click_button expects data (string) as third argument".into(),
            ));
        }
    };

    Ok((chat_id, message_id, data))
}

/// Return type for `parse_file_args`.
pub type FileArgs = (String, Option<String>, bool, Option<Vec<String>>);

/// Parses `file` arguments supporting both positional and single-table forms.
pub fn parse_file_args(arg1: Value, arg2: Option<Value>) -> Result<FileArgs, mlua::Error> {
    if let Value::Table(t) = &arg1
        && t.contains_key("prompt")?
    {
        let prompt: String = t.get("prompt")?;
        let default: Option<String> = t.get("default").ok();
        let must_exist: bool = t.get("must_exist").unwrap_or(true);
        let extensions = match t.get::<Value>("extensions").ok() {
            Some(Value::Table(ext_tbl)) => {
                let mut list = Vec::new();
                for item in ext_tbl.sequence_values::<String>().flatten() {
                    list.push(item);
                }
                Some(list)
            }
            Some(Value::String(s)) => {
                let s_str = s.to_str()?.to_string();
                Some(
                    s_str
                        .split(',')
                        .map(|x| x.trim().trim_start_matches('.').to_string())
                        .collect(),
                )
            }
            _ => None,
        };
        return Ok((prompt, default, must_exist, extensions));
    }

    let prompt = match arg1 {
        Value::String(s) => s.to_str()?.to_string(),
        _ => {
            return Err(mlua::Error::RuntimeError(
                "file requires prompt (string) or table with named parameters".into(),
            ));
        }
    };

    let mut default = None;
    let mut must_exist = true;
    let mut extensions = None;

    if let Some(second) = arg2 {
        match second {
            Value::String(s) => {
                default = Some(s.to_str()?.to_string());
            }
            Value::Table(t) => {
                default = t.get("default").ok();
                must_exist = t.get("must_exist").unwrap_or(true);
                extensions = match t.get::<Value>("extensions").ok() {
                    Some(Value::Table(ext_tbl)) => {
                        let mut list = Vec::new();
                        for item in ext_tbl.sequence_values::<String>().flatten() {
                            list.push(item);
                        }
                        Some(list)
                    }
                    Some(Value::String(s)) => {
                        let s_str = s.to_str()?.to_string();
                        Some(
                            s_str
                                .split(',')
                                .map(|x| x.trim().trim_start_matches('.').to_string())
                                .collect(),
                        )
                    }
                    _ => None,
                };
            }
            _ => {}
        }
    }

    Ok((prompt, default, must_exist, extensions))
}

/// Helper to parse select options from a Lua table.
pub fn parse_select_options(val: &Value) -> Result<(Vec<String>, Vec<String>), mlua::Error> {
    let t = match val {
        Value::Table(t) => t,
        _ => return Err(mlua::Error::RuntimeError("Options must be a table".into())),
    };

    let mut labels = Vec::new();
    let mut values = Vec::new();

    for pair in t.sequence_values::<Value>() {
        let item =
            pair.map_err(|e| mlua::Error::RuntimeError(format!("Invalid select option: {e}")))?;
        match item {
            Value::String(s) => {
                let s_str = s
                    .to_str()
                    .map_err(|e| mlua::Error::RuntimeError(e.to_string()))?
                    .to_string();
                labels.push(s_str.clone());
                values.push(s_str);
            }
            Value::Table(item_table) => {
                let label: String = item_table
                    .get("label")
                    .or_else(|_| item_table.get("name"))
                    .or_else(|_| item_table.get("text"))
                    .map_err(|_| {
                        mlua::Error::RuntimeError(
                            "Select option table must have 'label' or 'name'".into(),
                        )
                    })?;
                let val: String = item_table.get("value").unwrap_or_else(|_| label.clone());
                labels.push(label);
                values.push(val);
            }
            other => {
                let s = other
                    .to_string()
                    .map_err(|e| mlua::Error::RuntimeError(e.to_string()))?;
                labels.push(s.clone());
                values.push(s);
            }
        }
    }

    if labels.is_empty() {
        return Err(mlua::Error::RuntimeError(
            "Select options array cannot be empty".into(),
        ));
    }

    Ok((labels, values))
}

/// Helper to parse `ox.select` arguments, supporting both positional:
/// `(prompt, options, default)`
/// and single-table named form:
/// `{ prompt = "...", options = { ... }, default = "..." }`.
pub fn parse_select_args(
    arg1: Value,
    arg2: Option<Value>,
    arg3: Option<Value>,
) -> Result<(String, Value, Option<String>), mlua::Error> {
    if let Value::Table(ref t) = arg1
        && t.contains_key("prompt")?
        && (t.contains_key("options")? || t.contains_key("choices")?)
    {
        let p: String = t.get("prompt")?;
        let opts: Value = t
            .get("options")
            .or_else(|_| t.get("choices"))
            .map_err(|e| mlua::Error::RuntimeError(e.to_string()))?;
        let def: Option<String> = t.get("default").ok();
        return Ok((p, opts, def));
    }

    let p = match arg1 {
        Value::String(s) => s.to_str()?.to_string(),
        _ => {
            return Err(mlua::Error::RuntimeError(
                "select requires prompt string or options table".into(),
            ));
        }
    };
    let opts =
        arg2.ok_or_else(|| mlua::Error::RuntimeError("select requires options table".into()))?;
    let def = match arg3 {
        Some(Value::String(s)) => Some(s.to_str()?.to_string()),
        Some(Value::Integer(i)) => Some(i.to_string()),
        Some(Value::Number(n)) => Some(n.to_string()),
        _ => None,
    };
    Ok((p, opts, def))
}

/// Helper to parse `ox.confirm` arguments, supporting both positional:
/// `(prompt, default)`
/// and single-table named form:
/// `{ prompt = "...", default = true }`.
pub fn parse_confirm_args(
    arg1: Value,
    arg2: Option<bool>,
) -> Result<(String, Option<bool>), mlua::Error> {
    match arg1 {
        Value::Table(ref t) if t.contains_key("prompt")? => {
            let p: String = t.get("prompt")?;
            let def: Option<bool> = t.get("default").ok();
            Ok((p, def))
        }
        Value::String(s) => Ok((s.to_str()?.to_string(), arg2)),
        _ => Err(mlua::Error::RuntimeError(
            "confirm requires prompt string or table with 'prompt'".into(),
        )),
    }
}

/// Helper to extract formatted log message supporting both `ox.log.info("msg")`
/// and `ox.log:info("msg")`.
pub fn extract_log_msg(a1: Value, a2: Option<Value>) -> Result<String, mlua::Error> {
    let val = match (a1, a2) {
        (Value::Table(_), Some(v)) => v,
        (v, _) => v,
    };
    match val {
        Value::String(s) => Ok(s.to_str()?.to_string()),
        Value::Integer(i) => Ok(i.to_string()),
        Value::Number(n) => Ok(n.to_string()),
        Value::Boolean(b) => Ok(b.to_string()),
        _ => Ok(format!("{val:?}")),
    }
}

/// Invokes a raw MTProto request with automatic backoff retry on FloodWait.
pub async fn invoke_with_flood_wait<R: tl::RemoteCall>(
    client: &Client,
    request: &R,
) -> Result<R::Return, mlua::Error> {
    let mut attempts = 0;
    loop {
        attempts += 1;
        let res = tokio::time::timeout(NETWORK_TIMEOUT, client.invoke(request)).await;
        match res {
            Ok(Ok(val)) => return Ok(val),
            Ok(Err(grammers_client::InvocationError::Rpc(rpc_err))) => {
                if rpc_err.name == "FLOOD_WAIT" && attempts <= 3 {
                    let wait_secs = rpc_err.value.unwrap_or(1);
                    tracing::warn!(
                        wait_secs,
                        "Telegram FloodWait detected, backing off and retrying..."
                    );
                    tokio::time::sleep(std::time::Duration::from_secs(wait_secs as u64 + 1)).await;
                    continue;
                }
                return Err(mlua::Error::RuntimeError(format!(
                    "Telegram RPC error: {rpc_err}"
                )));
            }
            Ok(Err(e)) => {
                return Err(mlua::Error::RuntimeError(format!(
                    "Telegram invoke error: {e}"
                )));
            }
            Err(_) => {
                return Err(mlua::Error::RuntimeError(
                    "Telegram request timed out".into(),
                ));
            }
        }
    }
}

/// Sends a message with automatic backoff retry on FloodWait.
pub async fn invoke_send_message(
    client: &Client,
    peer_ref: PeerRef,
    msg: InputMessage,
) -> Result<grammers_client::message::Message, mlua::Error> {
    let mut attempts = 0;
    loop {
        attempts += 1;
        let res =
            tokio::time::timeout(NETWORK_TIMEOUT, client.send_message(peer_ref, msg.clone())).await;
        match res {
            Ok(Ok(m)) => return Ok(m),
            Ok(Err(grammers_client::InvocationError::Rpc(rpc_err))) => {
                if rpc_err.name == "FLOOD_WAIT" && attempts <= 3 {
                    let wait_secs = rpc_err.value.unwrap_or(1);
                    tracing::warn!(
                        wait_secs,
                        "Telegram FloodWait detected on send_message, backing off..."
                    );
                    tokio::time::sleep(std::time::Duration::from_secs(wait_secs as u64 + 1)).await;
                    continue;
                }
                return Err(mlua::Error::RuntimeError(format!(
                    "Telegram send error: {rpc_err}"
                )));
            }
            Ok(Err(e)) => {
                return Err(mlua::Error::RuntimeError(format!(
                    "Telegram send error: {e}"
                )));
            }
            Err(_) => {
                return Err(mlua::Error::RuntimeError(
                    "Telegram send message timed out".into(),
                ));
            }
        }
    }
}

/// Helper function to resolve a `PeerRef` from cache or fallback to dialog iteration.
pub async fn resolve_peer_ref(
    client: &Client,
    peer_refs: &Arc<RwLock<HashMap<i64, PeerRef>>>,
    chat_id: i64,
) -> Result<PeerRef, mlua::Error> {
    if let Some(&cached) = peer_refs.read().await.get(&chat_id) {
        return Ok(cached);
    }

    let mut dialogs = client.iter_dialogs();
    while let Ok(Some(dialog)) = tokio::time::timeout(NETWORK_TIMEOUT, dialogs.next())
        .await
        .map_err(|_| mlua::Error::RuntimeError("Dialog resolution timed out".into()))?
    {
        if let Some(id) = dialog.peer_id().bare_id() {
            let p_ref = dialog.peer_ref();
            peer_refs.write().await.insert(id, p_ref);
            if id == chat_id {
                return Ok(p_ref);
            }
        }
    }

    Err(mlua::Error::RuntimeError(format!(
        "Telegram peer {chat_id} is unknown; receive a message from it first"
    )))
}

/// Helper function to create a stop callback function.
pub fn create_stop_function(lua: &Lua, stop_tx: watch::Sender<bool>) -> mlua::Result<Function> {
    lua.create_function(move |_, ()| {
        stop_tx.send_replace(true);
        Ok(())
    })
}

/// Builds and registers the `ox` global table in the given Lua state.
#[allow(clippy::too_many_arguments)]
pub fn register_ox_table(
    lua: &Lua,
    client: Client,
    handlers: Arc<Mutex<Vec<RegisteredHandler>>>,
    peer_refs: Arc<RwLock<HashMap<i64, PeerRef>>>,
    console: Arc<dyn BotConsole>,
    stop_tx: watch::Sender<bool>,
    timer_hub: Arc<TimerHub>,
    storage: crate::infrastructure::lua::storage::BotStorage,
) -> Result<(), ScriptError> {
    let ox_table = lua.create_table().map_err(ScriptError::LuaError)?;

    // Register string extensions and global string helpers
    crate::infrastructure::lua::stdlib::register_stdlib(lua, Some(&ox_table))
        .map_err(ScriptError::LuaError)?;

    // Register randomization utilities
    crate::infrastructure::lua::stdlib::register_random_helpers(lua, &ox_table)
        .map_err(ScriptError::LuaError)?;

    // Register persistent key-value storage
    crate::infrastructure::lua::storage::register_storage_api(lua, &ox_table, storage)
        .map_err(ScriptError::LuaError)?;

    // ox.send_message(chat_id, text, [options_or_delay]) OR ox.send_message { ... }
    let client_clone = client.clone();
    let send_peer_refs = Arc::clone(&peer_refs);
    let send_msg_fn = lua
        .create_async_function(
            move |_, (arg1, arg2, arg3): (Value, Option<Value>, Option<Value>)| {
                let client = client_clone.clone();
                let peer_refs = Arc::clone(&send_peer_refs);
                async move {
                    let (chat_id, text, opts) = parse_send_message_args(arg1, arg2, arg3)?;
                    apply_optional_delay(opts.delay).await;

                    let peer_ref = resolve_peer_ref(&client, &peer_refs, chat_id).await?;
                    let input_msg = build_input_message(&text, opts.parse_mode.as_deref());

                    invoke_send_message(&client, peer_ref, input_msg).await?;
                    Ok(())
                }
            },
        )
        .map_err(ScriptError::LuaError)?;

    ox_table
        .set("send_message", send_msg_fn)
        .map_err(ScriptError::LuaError)?;

    // ox.click_button(chat_id, message_id, data) OR ox.click_button { ... }
    let client_clone = client.clone();
    let click_peer_refs = Arc::clone(&peer_refs);
    let click_btn_fn = lua
        .create_async_function(
            move |lua_ctx, (arg1, arg2, arg3): (Value, Option<Value>, Option<Value>)| {
                let client = client_clone.clone();
                let peer_refs = Arc::clone(&click_peer_refs);
                async move {
                    let (chat_id, message_id, data) = parse_click_button_args(arg1, arg2, arg3)?;
                    let peer_ref = resolve_peer_ref(&client, &peer_refs, chat_id).await?;
                    let input_peer = tl::enums::InputPeer::from(&peer_ref);
                    let req = tl::functions::messages::GetBotCallbackAnswer {
                        game: false,
                        peer: input_peer,
                        msg_id: message_id,
                        data: Some(data.into_bytes()),
                        password: None,
                    };
                    let answer = invoke_with_flood_wait(&client, &req).await?;
                    let tl::enums::messages::BotCallbackAnswer::Answer(ans) = answer;

                    let res = lua_ctx.create_table()?;
                    res.set("alert", ans.alert)?;
                    res.set("message", ans.message)?;
                    res.set("url", ans.url)?;
                    Ok(res)
                }
            },
        )
        .map_err(ScriptError::LuaError)?;
    ox_table
        .set("click_button", click_btn_fn)
        .map_err(ScriptError::LuaError)?;

    // ox.edit_message(chat_id, message_id, new_text, [options]) OR ox.edit_message { ... }
    let client_clone = client.clone();
    let edit_peer_refs = Arc::clone(&peer_refs);
    let edit_fn = lua
        .create_async_function(
            move |_, (a1, a2, a3, a4): (Value, Option<Value>, Option<Value>, Option<Value>)| {
                let client = client_clone.clone();
                let peer_refs = Arc::clone(&edit_peer_refs);
                async move {
                    let (chat_id, message_id, new_text, opts) =
                        parse_edit_message_args(a1, a2, a3, a4)?;
                    apply_optional_delay(opts.delay).await;

                    let peer_ref = resolve_peer_ref(&client, &peer_refs, chat_id).await?;
                    let input_msg = build_input_message(&new_text, opts.parse_mode.as_deref());

                    tokio::time::timeout(
                        NETWORK_TIMEOUT,
                        client.edit_message(peer_ref, message_id, input_msg),
                    )
                    .await
                    .map_err(|_| mlua::Error::RuntimeError("Edit message timed out".into()))?
                    .map_err(|e| mlua::Error::RuntimeError(format!("Edit message error: {e}")))?;

                    Ok(())
                }
            },
        )
        .map_err(ScriptError::LuaError)?;
    ox_table
        .set("edit_message", edit_fn)
        .map_err(ScriptError::LuaError)?;

    // ox.delete_message(chat_id, message_id_or_table, [delay]) OR ox.delete_message { ... }
    let client_clone = client.clone();
    let del_peer_refs = Arc::clone(&peer_refs);
    let del_fn = lua
        .create_async_function(
            move |_, (arg1, arg2, arg3): (Value, Option<Value>, Option<Value>)| {
                let client = client_clone.clone();
                let peer_refs = Arc::clone(&del_peer_refs);
                async move {
                    let (chat_id, ids, delay) = parse_delete_message_args(arg1, arg2, arg3)?;
                    apply_optional_delay(delay).await;

                    let peer_ref = resolve_peer_ref(&client, &peer_refs, chat_id).await?;
                    tokio::time::timeout(NETWORK_TIMEOUT, client.delete_messages(peer_ref, &ids))
                        .await
                        .map_err(|_| mlua::Error::RuntimeError("Delete messages timed out".into()))?
                        .map_err(|e| {
                            mlua::Error::RuntimeError(format!("Delete messages error: {e}"))
                        })?;

                    Ok(())
                }
            },
        )
        .map_err(ScriptError::LuaError)?;
    ox_table
        .set("delete_message", del_fn)
        .map_err(ScriptError::LuaError)?;

    // ox.react(chat_id, message_id, emoji, [delay]) OR ox.react { ... }
    let client_clone = client.clone();
    let react_peer_refs = Arc::clone(&peer_refs);
    let react_fn = lua
        .create_async_function(
            move |_, (a1, a2, a3, a4): (Value, Option<Value>, Option<Value>, Option<Value>)| {
                let client = client_clone.clone();
                let peer_refs = Arc::clone(&react_peer_refs);
                async move {
                    let (chat_id, message_id, emoji, delay) = parse_react_args(a1, a2, a3, a4)?;
                    apply_optional_delay(delay).await;

                    let peer_ref = resolve_peer_ref(&client, &peer_refs, chat_id).await?;
                    tokio::time::timeout(
                        NETWORK_TIMEOUT,
                        client.send_reactions(peer_ref, message_id, emoji.as_str()),
                    )
                    .await
                    .map_err(|_| mlua::Error::RuntimeError("Send reaction timed out".into()))?
                    .map_err(|e| mlua::Error::RuntimeError(format!("Send reaction error: {e}")))?;

                    Ok(())
                }
            },
        )
        .map_err(ScriptError::LuaError)?;
    ox_table
        .set("react", react_fn)
        .map_err(ScriptError::LuaError)?;

    // ox.pin_message(chat_id, message_id, [delay]) OR ox.pin_message { ... }
    let client_clone = client.clone();
    let pin_peer_refs = Arc::clone(&peer_refs);
    let pin_fn = lua
        .create_async_function(
            move |_, (arg1, arg2, arg3): (Value, Option<Value>, Option<Value>)| {
                let client = client_clone.clone();
                let peer_refs = Arc::clone(&pin_peer_refs);
                async move {
                    let (chat_id, message_id, delay) = parse_pin_args(arg1, arg2, arg3)?;
                    apply_optional_delay(delay).await;

                    let peer_ref = resolve_peer_ref(&client, &peer_refs, chat_id).await?;
                    tokio::time::timeout(NETWORK_TIMEOUT, client.pin_message(peer_ref, message_id))
                        .await
                        .map_err(|_| mlua::Error::RuntimeError("Pin message timed out".into()))?
                        .map_err(|e| {
                            mlua::Error::RuntimeError(format!("Pin message error: {e}"))
                        })?;

                    Ok(())
                }
            },
        )
        .map_err(ScriptError::LuaError)?;
    ox_table
        .set("pin_message", pin_fn)
        .map_err(ScriptError::LuaError)?;

    // ox.forward_message(to_chat_id, from_chat_id, message_id) OR ox.forward_message { ... }
    let client_clone = client.clone();
    let fwd_peer_refs = Arc::clone(&peer_refs);
    let fwd_fn = lua
        .create_async_function(
            move |_, (arg1, arg2, arg3): (Value, Option<Value>, Option<Value>)| {
                let client = client_clone.clone();
                let peer_refs = Arc::clone(&fwd_peer_refs);
                async move {
                    let (to_chat_id, from_chat_id, message_id) =
                        parse_forward_args(arg1, arg2, arg3)?;
                    let to_peer = resolve_peer_ref(&client, &peer_refs, to_chat_id).await?;
                    let from_peer = resolve_peer_ref(&client, &peer_refs, from_chat_id).await?;
                    tokio::time::timeout(
                        NETWORK_TIMEOUT,
                        client.forward_messages(to_peer, &[message_id], from_peer),
                    )
                    .await
                    .map_err(|_| mlua::Error::RuntimeError("Forward messages timed out".into()))?
                    .map_err(|e| {
                        mlua::Error::RuntimeError(format!("Forward messages error: {e}"))
                    })?;

                    Ok(())
                }
            },
        )
        .map_err(ScriptError::LuaError)?;
    ox_table
        .set("forward_message", fwd_fn)
        .map_err(ScriptError::LuaError)?;

    // ox.send_image(chat_id, path, [caption], [options]) OR ox.send_image { ... }
    let client_clone = client.clone();
    let image_peer_refs = Arc::clone(&peer_refs);
    let send_image_fn = lua
        .create_async_function(
            move |_, (a1, a2, a3, a4): (Value, Option<Value>, Option<Value>, Option<Value>)| {
                let client = client_clone.clone();
                let peer_refs = Arc::clone(&image_peer_refs);
                async move {
                    let (chat_id, path, caption, opts) =
                        parse_media_send_args("send_image", a1, a2, a3, a4)?;
                    apply_optional_delay(opts.delay).await;

                    let uploaded = tokio::time::timeout(NETWORK_TIMEOUT, client.upload_file(&path))
                        .await
                        .map_err(|_| {
                            mlua::Error::RuntimeError("Telegram upload file timed out".into())
                        })?
                        .map_err(|error| {
                            mlua::Error::RuntimeError(format!(
                                "Failed to upload image '{path}': {error}"
                            ))
                        })?;

                    let message = if let Some(c) = caption {
                        build_input_message(&c, opts.parse_mode.as_deref()).photo(uploaded)
                    } else {
                        InputMessage::new().photo(uploaded)
                    };

                    let peer_ref = resolve_peer_ref(&client, &peer_refs, chat_id).await?;
                    invoke_send_message(&client, peer_ref, message).await?;
                    Ok(())
                }
            },
        )
        .map_err(ScriptError::LuaError)?;
    ox_table
        .set("send_image", send_image_fn)
        .map_err(ScriptError::LuaError)?;

    // ox.send_document(chat_id, path, [caption], [options]) OR ox.send_document { ... }
    let client_clone = client.clone();
    let doc_peer_refs = Arc::clone(&peer_refs);
    let send_doc_fn = lua
        .create_async_function(
            move |_, (a1, a2, a3, a4): (Value, Option<Value>, Option<Value>, Option<Value>)| {
                let client = client_clone.clone();
                let peer_refs = Arc::clone(&doc_peer_refs);
                async move {
                    let (chat_id, path, caption, opts) =
                        parse_media_send_args("send_document", a1, a2, a3, a4)?;
                    apply_optional_delay(opts.delay).await;

                    let uploaded = tokio::time::timeout(NETWORK_TIMEOUT, client.upload_file(&path))
                        .await
                        .map_err(|_| mlua::Error::RuntimeError("Upload document timed out".into()))?
                        .map_err(|e| {
                            mlua::Error::RuntimeError(format!("Upload document error: {e}"))
                        })?;

                    let msg = if let Some(c) = caption {
                        build_input_message(&c, opts.parse_mode.as_deref()).document(uploaded)
                    } else {
                        InputMessage::new().document(uploaded)
                    };

                    let peer_ref = resolve_peer_ref(&client, &peer_refs, chat_id).await?;
                    invoke_send_message(&client, peer_ref, msg).await?;
                    Ok(())
                }
            },
        )
        .map_err(ScriptError::LuaError)?;
    ox_table
        .set("send_document", send_doc_fn)
        .map_err(ScriptError::LuaError)?;

    // ox.send_audio(chat_id, path, [caption], [options]) OR ox.send_audio { ... }
    let client_clone = client.clone();
    let audio_peer_refs = Arc::clone(&peer_refs);
    let send_audio_fn = lua
        .create_async_function(
            move |_, (a1, a2, a3, a4): (Value, Option<Value>, Option<Value>, Option<Value>)| {
                let client = client_clone.clone();
                let peer_refs = Arc::clone(&audio_peer_refs);
                async move {
                    let (chat_id, path, caption, opts) =
                        parse_media_send_args("send_audio", a1, a2, a3, a4)?;
                    apply_optional_delay(opts.delay).await;

                    let uploaded = tokio::time::timeout(NETWORK_TIMEOUT, client.upload_file(&path))
                        .await
                        .map_err(|_| mlua::Error::RuntimeError("Upload audio timed out".into()))?
                        .map_err(|e| {
                            mlua::Error::RuntimeError(format!("Upload audio error: {e}"))
                        })?;

                    let msg = if let Some(c) = caption {
                        build_input_message(&c, opts.parse_mode.as_deref()).file(uploaded)
                    } else {
                        InputMessage::new().file(uploaded)
                    };

                    let peer_ref = resolve_peer_ref(&client, &peer_refs, chat_id).await?;
                    invoke_send_message(&client, peer_ref, msg).await?;
                    Ok(())
                }
            },
        )
        .map_err(ScriptError::LuaError)?;
    ox_table
        .set("send_audio", send_audio_fn)
        .map_err(ScriptError::LuaError)?;

    // ox.send_voice(chat_id, path, [delay]) OR ox.send_voice { ... }
    let client_clone = client.clone();
    let voice_peer_refs = Arc::clone(&peer_refs);
    let send_voice_fn = lua
        .create_async_function(
            move |_, (a1, a2, a3): (Value, Option<Value>, Option<Value>)| {
                let client = client_clone.clone();
                let peer_refs = Arc::clone(&voice_peer_refs);
                async move {
                    let (chat_id, path, _, opts) =
                        parse_media_send_args("send_voice", a1, a2, None, a3)?;
                    apply_optional_delay(opts.delay).await;

                    let uploaded = tokio::time::timeout(NETWORK_TIMEOUT, client.upload_file(&path))
                        .await
                        .map_err(|_| mlua::Error::RuntimeError("Upload voice timed out".into()))?
                        .map_err(|e| {
                            mlua::Error::RuntimeError(format!("Upload voice error: {e}"))
                        })?;

                    let msg = InputMessage::new().file(uploaded).mime_type("audio/ogg");
                    let peer_ref = resolve_peer_ref(&client, &peer_refs, chat_id).await?;
                    invoke_send_message(&client, peer_ref, msg).await?;
                    Ok(())
                }
            },
        )
        .map_err(ScriptError::LuaError)?;
    ox_table
        .set("send_voice", send_voice_fn)
        .map_err(ScriptError::LuaError)?;

    // ox.send_typing(chat_id)
    let client_clone = client.clone();
    let typing_peer_refs = Arc::clone(&peer_refs);
    let typing_fn = lua
        .create_async_function(move |_, chat_id: i64| {
            let client = client_clone.clone();
            let peer_refs = Arc::clone(&typing_peer_refs);
            async move {
                let peer_ref = resolve_peer_ref(&client, &peer_refs, chat_id).await?;
                let input_peer = tl::enums::InputPeer::from(&peer_ref);
                let req = tl::functions::messages::SetTyping {
                    peer: input_peer,
                    top_msg_id: None,
                    action: tl::enums::SendMessageAction::SendMessageTypingAction,
                };
                let _ = invoke_with_flood_wait(&client, &req).await;
                Ok(())
            }
        })
        .map_err(ScriptError::LuaError)?;
    ox_table
        .set("send_typing", typing_fn)
        .map_err(ScriptError::LuaError)?;

    // ox.set_interval(seconds, callback)
    let timer_hub_clone = Arc::clone(&timer_hub);
    let set_interval_fn = lua
        .create_function(move |lua_ctx, (seconds, callback): (f64, Function)| {
            let key = lua_ctx.create_registry_value(callback)?;
            let hub = Arc::clone(&timer_hub_clone);
            let duration = std::time::Duration::from_secs_f64(seconds.max(0.1));
            let id = tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(hub.add_timer(key, duration, true))
            });
            Ok(id)
        })
        .map_err(ScriptError::LuaError)?;
    ox_table
        .set("set_interval", set_interval_fn)
        .map_err(ScriptError::LuaError)?;

    // ox.set_timeout(seconds, callback)
    let timer_hub_clone = Arc::clone(&timer_hub);
    let set_timeout_fn = lua
        .create_function(move |lua_ctx, (seconds, callback): (f64, Function)| {
            let key = lua_ctx.create_registry_value(callback)?;
            let hub = Arc::clone(&timer_hub_clone);
            let duration = std::time::Duration::from_secs_f64(seconds.max(0.01));
            let id = tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(hub.add_timer(key, duration, false))
            });
            Ok(id)
        })
        .map_err(ScriptError::LuaError)?;
    ox_table
        .set("set_timeout", set_timeout_fn)
        .map_err(ScriptError::LuaError)?;

    // ox.clear_timer(timer_id)
    let timer_hub_clone = Arc::clone(&timer_hub);
    let clear_timer_fn = lua
        .create_function(move |_, id: u64| {
            let hub = Arc::clone(&timer_hub_clone);
            let cleared = tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(hub.clear_timer(id))
            });
            Ok(cleared)
        })
        .map_err(ScriptError::LuaError)?;
    ox_table
        .set("clear_timer", clear_timer_fn.clone())
        .map_err(ScriptError::LuaError)?;
    ox_table
        .set("clear_interval", clear_timer_fn.clone())
        .map_err(ScriptError::LuaError)?;
    ox_table
        .set("clear_timeout", clear_timer_fn)
        .map_err(ScriptError::LuaError)?;

    // ox.sleep(seconds)
    let sleep_fn = lua
        .create_async_function(|_, secs: f64| async move {
            tokio::time::sleep(std::time::Duration::from_secs_f64(secs.max(0.0))).await;
            Ok(())
        })
        .map_err(ScriptError::LuaError)?;
    ox_table
        .set("sleep", sleep_fn)
        .map_err(ScriptError::LuaError)?;

    // ox.log table
    let log_table = lua.create_table().map_err(ScriptError::LuaError)?;

    let log_info = lua
        .create_function(move |_, (a1, a2): (Value, Option<Value>)| {
            let msg = extract_log_msg(a1, a2)?;
            tracing::info!(target: "lua", "[LUA] {msg}");
            Ok(())
        })
        .map_err(ScriptError::LuaError)?;
    let log_warn = lua
        .create_function(move |_, (a1, a2): (Value, Option<Value>)| {
            let msg = extract_log_msg(a1, a2)?;
            tracing::warn!(target: "lua", "[LUA] {msg}");
            Ok(())
        })
        .map_err(ScriptError::LuaError)?;
    let log_error = lua
        .create_function(move |_, (a1, a2): (Value, Option<Value>)| {
            let msg = extract_log_msg(a1, a2)?;
            tracing::error!(target: "lua", "[LUA] {msg}");
            Ok(())
        })
        .map_err(ScriptError::LuaError)?;
    let log_debug = lua
        .create_function(move |_, (a1, a2): (Value, Option<Value>)| {
            let msg = extract_log_msg(a1, a2)?;
            tracing::debug!(target: "lua", "[LUA] {msg}");
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
    ox_table
        .set("log", log_table)
        .map_err(ScriptError::LuaError)?;

    // ox.select(prompt, options_table, [default]) OR ox.select { prompt = ..., options = ..., default = ... }
    let console_clone = Arc::clone(&console);
    let select_fn = lua
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
    ox_table
        .set("select", select_fn)
        .map_err(ScriptError::LuaError)?;

    // ox.confirm(prompt, [default]) OR ox.confirm { prompt = ..., default = ... }
    let console_clone = Arc::clone(&console);
    let confirm_fn = lua
        .create_function(move |_, (arg1, arg2): (Value, Option<bool>)| {
            let (prompt, default) = parse_confirm_args(arg1, arg2)?;
            console_clone
                .ask_confirm(&prompt, default)
                .map_err(mlua::Error::RuntimeError)
        })
        .map_err(ScriptError::LuaError)?;
    ox_table
        .set("confirm", confirm_fn)
        .map_err(ScriptError::LuaError)?;

    // ox.input(prompt, [default]) OR ox.input { prompt = ..., default = ... }
    let console_clone = Arc::clone(&console);
    let input_fn = lua
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
    ox_table
        .set("input", input_fn)
        .map_err(ScriptError::LuaError)?;

    // ox.file(prompt, [default_or_options]) OR ox.file { ... }
    let console_clone = Arc::clone(&console);
    let file_fn = lua
        .create_function(move |_, (arg1, arg2): (Value, Option<Value>)| {
            let (prompt, default, must_exist, extensions) = parse_file_args(arg1, arg2)?;
            console_clone
                .ask_file(&prompt, default.as_deref(), must_exist, extensions)
                .map_err(mlua::Error::RuntimeError)
        })
        .map_err(ScriptError::LuaError)?;
    ox_table
        .set("file", file_fn)
        .map_err(ScriptError::LuaError)?;

    // ox.stop()
    let stop_fn = create_stop_function(lua, stop_tx).map_err(ScriptError::LuaError)?;
    ox_table
        .set("stop", stop_fn)
        .map_err(ScriptError::LuaError)?;

    // ox.on_message(filters_or_func, optional_func)
    let register_handler_fn = lua
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

    // Register FSM dialog flow constructor
    crate::infrastructure::lua::flow::register_flow_api(lua, &ox_table)
        .map_err(ScriptError::LuaError)?;

    let globals = lua.globals();
    globals.set("ox", ox_table).map_err(ScriptError::LuaError)?;

    Ok(())
}
