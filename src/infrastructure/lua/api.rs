//! Registration and bindings for global `ox.*` Lua API methods.

use crate::application::automation::BotConsole;
use crate::domain::automation::MessageFilter;
use crate::errors::ScriptError;
use crate::infrastructure::lua::filters::parse_message_filter;
use grammers_client::Client;
use grammers_client::message::InputMessage;
use grammers_session::types::PeerRef;
use mlua::{Function, Lua, RegistryKey, Value};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock, watch};

/// Registered message handler containing callback key and optional filter.
pub struct RegisteredHandler {
    pub callback_key: RegistryKey,
    pub filter: Option<MessageFilter>,
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
    while let Ok(Some(dialog)) = dialogs.next().await {
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
pub fn register_ox_table(
    lua: &Lua,
    client: Client,
    handlers: Arc<Mutex<Vec<RegisteredHandler>>>,
    peer_refs: Arc<RwLock<HashMap<i64, PeerRef>>>,
    console: Arc<dyn BotConsole>,
    stop_tx: watch::Sender<bool>,
) -> Result<(), ScriptError> {
    let ox_table = lua.create_table().map_err(ScriptError::LuaError)?;

    // ox.send_message(chat_id, text, delay_sec)
    let client_clone = client.clone();
    let send_peer_refs = Arc::clone(&peer_refs);
    let send_msg_fn = lua
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

                    let peer_ref = resolve_peer_ref(&client, &peer_refs, chat_id).await?;

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
    let send_image_fn = lua
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
                    let peer_ref = resolve_peer_ref(&client, &peer_refs, chat_id).await?;

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
    let console_clone = Arc::clone(&console);
    let input_fn = lua
        .create_function(move |_, (prompt, default): (String, Option<String>)| {
            console_clone
                .ask_input(&prompt, default.as_deref())
                .map_err(mlua::Error::RuntimeError)
        })
        .map_err(ScriptError::LuaError)?;
    ox_table
        .set("input", input_fn)
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

    let globals = lua.globals();
    globals
        .set("ox", ox_table)
        .map_err(ScriptError::LuaError)?;

    Ok(())
}
