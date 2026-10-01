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
use mlua::{Function, Lua, RegistryKey, Value};
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
    crate::infrastructure::lua::stdlib::register_stdlib(lua).map_err(ScriptError::LuaError)?;

    // Register randomization utilities
    crate::infrastructure::lua::stdlib::register_random_helpers(lua, &ox_table)
        .map_err(ScriptError::LuaError)?;

    // Register persistent key-value storage
    crate::infrastructure::lua::storage::register_storage_api(lua, &ox_table, storage)
        .map_err(ScriptError::LuaError)?;

    // ox.send_message(chat_id, text, [options_or_delay], [extra_delay])
    let client_clone = client.clone();
    let send_peer_refs = Arc::clone(&peer_refs);
    let send_msg_fn = lua
        .create_async_function(
            move |_,
                  (chat_id, text, opt_val, extra_delay): (
                i64,
                String,
                Option<Value>,
                Option<f64>,
            )| {
                let client = client_clone.clone();
                let peer_refs = Arc::clone(&send_peer_refs);
                async move {
                    let mut opts = parse_message_options(opt_val)?;
                    if extra_delay.is_some() {
                        opts.delay = extra_delay;
                    }
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
        .set("send_message", send_msg_fn.clone())
        .map_err(ScriptError::LuaError)?;
    ox_table
        .set("reply", send_msg_fn)
        .map_err(ScriptError::LuaError)?;

    // ox.click_button(chat_id, message_id, data)
    let client_clone = client.clone();
    let click_peer_refs = Arc::clone(&peer_refs);
    let click_btn_fn = lua
        .create_async_function(
            move |lua_ctx, (chat_id, message_id, data): (i64, i32, String)| {
                let client = client_clone.clone();
                let peer_refs = Arc::clone(&click_peer_refs);
                async move {
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

    // ox.edit_message(chat_id, message_id, new_text, [options])
    let client_clone = client.clone();
    let edit_peer_refs = Arc::clone(&peer_refs);
    let edit_fn =
        lua
            .create_async_function(
                move |_,
                      (chat_id, message_id, new_text, options): (
                    i64,
                    i32,
                    String,
                    Option<Value>,
                )| {
                    let client = client_clone.clone();
                    let peer_refs = Arc::clone(&edit_peer_refs);
                    async move {
                        let opts = parse_message_options(options)?;
                        apply_optional_delay(opts.delay).await;

                        let peer_ref = resolve_peer_ref(&client, &peer_refs, chat_id).await?;
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
    ox_table
        .set("edit_message", edit_fn)
        .map_err(ScriptError::LuaError)?;

    // ox.delete_message(chat_id, message_id_or_table)
    let client_clone = client.clone();
    let del_peer_refs = Arc::clone(&peer_refs);
    let del_fn = lua
        .create_async_function(move |_, (chat_id, ids_val): (i64, Value)| {
            let client = client_clone.clone();
            let peer_refs = Arc::clone(&del_peer_refs);
            async move {
                let ids: Vec<i32> = match ids_val {
                    Value::Integer(id) => vec![id as i32],
                    Value::Number(n) => vec![n as i32],
                    Value::Table(t) => {
                        let mut list = Vec::new();
                        for pair in t.sequence_values::<i32>() {
                            list.push(pair?);
                        }
                        list
                    }
                    _ => {
                        return Err(mlua::Error::RuntimeError(
                            "Expected message ID or array of IDs".into(),
                        ));
                    }
                };

                let peer_ref = resolve_peer_ref(&client, &peer_refs, chat_id).await?;
                tokio::time::timeout(NETWORK_TIMEOUT, client.delete_messages(peer_ref, &ids))
                    .await
                    .map_err(|_| mlua::Error::RuntimeError("Delete messages timed out".into()))?
                    .map_err(|e| {
                        mlua::Error::RuntimeError(format!("Delete messages error: {e}"))
                    })?;

                Ok(())
            }
        })
        .map_err(ScriptError::LuaError)?;
    ox_table
        .set("delete_message", del_fn.clone())
        .map_err(ScriptError::LuaError)?;
    ox_table
        .set("delete_messages", del_fn)
        .map_err(ScriptError::LuaError)?;

    // ox.send_reaction(chat_id, message_id, emoji)
    let client_clone = client.clone();
    let react_peer_refs = Arc::clone(&peer_refs);
    let react_fn = lua
        .create_async_function(move |_, (chat_id, message_id, emoji): (i64, i32, String)| {
            let client = client_clone.clone();
            let peer_refs = Arc::clone(&react_peer_refs);
            async move {
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
        })
        .map_err(ScriptError::LuaError)?;
    ox_table
        .set("send_reaction", react_fn.clone())
        .map_err(ScriptError::LuaError)?;
    ox_table
        .set("react", react_fn)
        .map_err(ScriptError::LuaError)?;

    // ox.pin_message(chat_id, message_id)
    let client_clone = client.clone();
    let pin_peer_refs = Arc::clone(&peer_refs);
    let pin_fn = lua
        .create_async_function(move |_, (chat_id, message_id): (i64, i32)| {
            let client = client_clone.clone();
            let peer_refs = Arc::clone(&pin_peer_refs);
            async move {
                let peer_ref = resolve_peer_ref(&client, &peer_refs, chat_id).await?;
                tokio::time::timeout(NETWORK_TIMEOUT, client.pin_message(peer_ref, message_id))
                    .await
                    .map_err(|_| mlua::Error::RuntimeError("Pin message timed out".into()))?
                    .map_err(|e| mlua::Error::RuntimeError(format!("Pin message error: {e}")))?;

                Ok(())
            }
        })
        .map_err(ScriptError::LuaError)?;
    ox_table
        .set("pin_message", pin_fn)
        .map_err(ScriptError::LuaError)?;

    // ox.forward_message(to_chat_id, from_chat_id, message_id)
    let client_clone = client.clone();
    let fwd_peer_refs = Arc::clone(&peer_refs);
    let fwd_fn = lua
        .create_async_function(
            move |_, (to_chat_id, from_chat_id, message_id): (i64, i64, i32)| {
                let client = client_clone.clone();
                let peer_refs = Arc::clone(&fwd_peer_refs);
                async move {
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

    // ox.send_image(chat_id, path, optional_caption, optional_options)
    let client_clone = client.clone();
    let image_peer_refs = Arc::clone(&peer_refs);
    let send_image_fn = lua
        .create_async_function(
            move |_,
                  (chat_id, path, caption, opt_val): (
                i64,
                String,
                Option<String>,
                Option<Value>,
            )| {
                let client = client_clone.clone();
                let peer_refs = Arc::clone(&image_peer_refs);
                async move {
                    let opts = parse_message_options(opt_val)?;
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

    // ox.send_document(chat_id, path, [caption], [options])
    let client_clone = client.clone();
    let doc_peer_refs = Arc::clone(&peer_refs);
    let send_doc_fn = lua
        .create_async_function(
            move |_,
                  (chat_id, path, caption, opt_val): (
                i64,
                String,
                Option<String>,
                Option<Value>,
            )| {
                let client = client_clone.clone();
                let peer_refs = Arc::clone(&doc_peer_refs);
                async move {
                    let opts = parse_message_options(opt_val)?;
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

    // ox.send_audio(chat_id, path, [caption], [options])
    let client_clone = client.clone();
    let audio_peer_refs = Arc::clone(&peer_refs);
    let send_audio_fn = lua
        .create_async_function(
            move |_,
                  (chat_id, path, caption, opt_val): (
                i64,
                String,
                Option<String>,
                Option<Value>,
            )| {
                let client = client_clone.clone();
                let peer_refs = Arc::clone(&audio_peer_refs);
                async move {
                    let opts = parse_message_options(opt_val)?;
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

    // ox.send_voice(chat_id, path, [delay])
    let client_clone = client.clone();
    let voice_peer_refs = Arc::clone(&peer_refs);
    let send_voice_fn = lua
        .create_async_function(
            move |_, (chat_id, path, delay): (i64, String, Option<f64>)| {
                let client = client_clone.clone();
                let peer_refs = Arc::clone(&voice_peer_refs);
                async move {
                    apply_optional_delay(delay).await;

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
            // Add timer to hub
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
        .set("clear_timer", clear_timer_fn)
        .map_err(ScriptError::LuaError)?;

    // ox.log table
    let log_table = lua.create_table().map_err(ScriptError::LuaError)?;
    let log_info = lua
        .create_function(|_, msg: String| {
            tracing::info!(target: "lua", "[LUA] {msg}");
            Ok(())
        })
        .map_err(ScriptError::LuaError)?;
    let log_warn = lua
        .create_function(|_, msg: String| {
            tracing::warn!(target: "lua", "[LUA] {msg}");
            Ok(())
        })
        .map_err(ScriptError::LuaError)?;
    let log_error = lua
        .create_function(|_, msg: String| {
            tracing::error!(target: "lua", "[LUA] {msg}");
            Ok(())
        })
        .map_err(ScriptError::LuaError)?;
    let log_debug = lua
        .create_function(|_, msg: String| {
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

    // Helper to parse select options from a Lua table
    fn parse_select_options(val: &Value) -> Result<(Vec<String>, Vec<String>), mlua::Error> {
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

    // ox.select(prompt, options_table, [default])
    let console_clone = Arc::clone(&console);
    let select_fn = lua
        .create_function(
            move |_, (prompt, options_val, default): (String, Value, Option<String>)| {
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

    // ox.confirm(prompt, [default])
    let console_clone = Arc::clone(&console);
    let confirm_fn = lua
        .create_function(move |_, (prompt, default): (String, Option<bool>)| {
            console_clone
                .ask_confirm(&prompt, default)
                .map_err(mlua::Error::RuntimeError)
        })
        .map_err(ScriptError::LuaError)?;
    ox_table
        .set("confirm", confirm_fn)
        .map_err(ScriptError::LuaError)?;

    // ox.input(prompt, [default_or_config])
    let console_clone = Arc::clone(&console);
    let input_fn = lua
        .create_function(
            move |_, (prompt, second_arg): (String, Option<Value>)| match second_arg {
                Some(Value::String(s)) => {
                    let s_str = s
                        .to_str()
                        .map_err(|e| mlua::Error::RuntimeError(e.to_string()))?;
                    console_clone
                        .ask_input(&prompt, Some(&s_str))
                        .map_err(mlua::Error::RuntimeError)
                }
                Some(Value::Table(cfg)) => {
                    if let Ok(options_val) = cfg.get::<Value>("options") {
                        let (labels, values) = parse_select_options(&options_val)?;
                        let default: Option<String> = cfg.get("default").ok();
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
                    } else {
                        let default: Option<String> = cfg.get("default").ok();
                        console_clone
                            .ask_input(&prompt, default.as_deref())
                            .map_err(mlua::Error::RuntimeError)
                    }
                }
                None => console_clone
                    .ask_input(&prompt, None)
                    .map_err(mlua::Error::RuntimeError),
                Some(other) => {
                    let s = other
                        .to_string()
                        .map_err(|e| mlua::Error::RuntimeError(e.to_string()))?;
                    console_clone
                        .ask_input(&prompt, Some(&s))
                        .map_err(mlua::Error::RuntimeError)
                }
            },
        )
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

    // Register FSM dialog flow constructor
    crate::infrastructure::lua::flow::register_flow_api(lua, &ox_table)
        .map_err(ScriptError::LuaError)?;

    let globals = lua.globals();
    globals
        .set("ox", ox_table.clone())
        .map_err(ScriptError::LuaError)?;
    globals
        .set("nox", ox_table)
        .map_err(ScriptError::LuaError)?;

    Ok(())
}
