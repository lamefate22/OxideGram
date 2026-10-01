//! Lua runtime and scripting infrastructure for OxideGram.

pub mod api;
pub mod buttons;
pub mod filters;
pub mod runner;

#[allow(unused_imports)]
pub use api::RegisteredHandler;
pub use runner::LuaBotRunner;

#[cfg(test)]
mod tests {
    use super::api::create_stop_function;
    use super::filters::parse_message_filter;
    use crate::domain::automation::{ChatType, MessageContext};
    use crate::domain::types::{ChatId, SenderId};
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
            chat_id: ChatId::new(100),
            sender_id: SenderId::new(42),
            incoming: true,
            chat_type: ChatType::Private,
        }));
        assert!(!filter.matches(&MessageContext {
            text: "/start payload",
            chat_id: ChatId::new(300),
            sender_id: SenderId::new(42),
            incoming: true,
            chat_type: ChatType::Private,
        }));
        assert!(!filter.matches(&MessageContext {
            text: "/start payload",
            chat_id: ChatId::new(100),
            sender_id: SenderId::new(42),
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
            chat_id: ChatId::new(1),
            sender_id: SenderId::new(1),
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

        lua.load(include_str!("../../../data/bots/anonchat.lua"))
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

    #[test]
    fn parses_various_message_options() {
        let lua = Lua::new();

        // None
        let opts = super::api::parse_message_options(None).unwrap();
        assert!(opts.delay.is_none());
        assert!(opts.parse_mode.is_none());

        // Numeric delay
        let val: Value = lua.load("2.5").eval().unwrap();
        let opts = super::api::parse_message_options(Some(val)).unwrap();
        assert_eq!(opts.delay, Some(2.5));
        assert!(opts.parse_mode.is_none());

        // Table with delay and markdown
        let val: Value = lua
            .load(r#"{ delay = 1.2, parse_mode = "markdown" }"#)
            .eval()
            .unwrap();
        let opts = super::api::parse_message_options(Some(val)).unwrap();
        assert_eq!(opts.delay, Some(1.2));
        assert_eq!(opts.parse_mode.as_deref(), Some("markdown"));
    }

    #[tokio::test]
    async fn timer_hub_schedules_and_cancels_properly() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(10);
        let hub = super::api::TimerHub::new(tx);
        let lua = Lua::new();

        let func = lua.create_function(|_, ()| Ok(())).unwrap();
        let key = lua.create_registry_value(func).unwrap();

        // Add timeout of 50ms
        let _id = hub
            .add_timer(key, std::time::Duration::from_millis(50), false)
            .await;

        let res = tokio::time::timeout(std::time::Duration::from_millis(200), rx.recv()).await;
        assert!(res.is_ok());

        // Test cancellation
        let func2 = lua.create_function(|_, ()| Ok(())).unwrap();
        let key2 = lua.create_registry_value(func2).unwrap();
        let id2 = hub
            .add_timer(key2, std::time::Duration::from_millis(200), true)
            .await;

        let cleared = hub.clear_timer(id2).await;
        assert!(cleared);

        // Ensure no events arrive for canceled timer
        let res2 = tokio::time::timeout(std::time::Duration::from_millis(250), rx.recv()).await;
        assert!(res2.is_err());
    }
}
