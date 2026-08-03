---
layout: default
title: Lua API Reference
---

# Lua API Reference

[Documentation home](index.html) | [Filters](filters.html) | [Examples](examples.html)

## Global Tables

### `ox`

The main OxideGram API table.

### `nox`

A compatibility alias that points to the same table as `ox`. Prefer `ox` in new scripts.

## Registering Handlers

### `ox.on_message(callback)`

Registers a callback for every new message:

```lua
ox.on_message(function(event)
    print(event.text)
end)
```

### `ox.on_message(filters, callback)`

Registers a callback for messages matching all supplied filters:

```lua
ox.on_message({
    incoming = true,
    private = true,
    commands = "start"
}, function(event)
    ox.send_message(event.chat_id, "Welcome!", 0)
end)
```

Filter tables are validated while the script loads. Invalid types, empty arrays, and invalid regular expressions stop script loading with an error. Unknown keys are currently ignored, so use the names from the [filter reference](filters.html).

Handlers run in registration order. Multiple matching handlers receive the same message. There is currently no propagation-stop mechanism.

## Sending Messages

### `ox.send_message(chat_id, text[, delay_sec])`

Sends a text message.

| Parameter | Type | Description |
| --- | --- | --- |
| `chat_id` | integer | Destination ID, usually `event.chat_id`. |
| `text` | string | Message text. |
| `delay_sec` | number or nil | Optional delay before sending, in seconds. Fractions are supported. Non-positive values send immediately. |

```lua
ox.send_message(event.chat_id, "Immediate", 0)
ox.send_message(event.chat_id, "After 1.5 seconds", 1.5)
ox.send_message(event.chat_id, "Delay omitted")
```

`ox.reply` is currently an alias with the same signature:

```lua
ox.reply(event.chat_id, "Reply text", 0)
```

At present, sending is most reliable when the destination is a private user ID. The current engine constructs a user peer reference from the numeric ID and does not resolve arbitrary groups or channels before sending.

## Event Table

Each message callback receives one `event` table.

| Field | Type | Description |
| --- | --- | --- |
| `event.text` | string | Message text. Empty when the update has no textual content. |
| `event.chat_id` | integer | Bare numeric ID of the dialog where the message appeared. |
| `event.sender_id` | integer | Bare sender ID. It is `0` when Telegram does not provide a resolvable sender. |
| `event.incoming` | boolean | `true` for messages not sent by the authorized account. |
| `event.outgoing` | boolean | `true` for messages sent by the authorized account. |
| `event.is_private` | boolean | The dialog peer is a user. |
| `event.is_group` | boolean | The dialog peer is a basic Telegram group. |
| `event.is_channel` | boolean | The dialog peer is a channel or supergroup. |
| `event.reply` | function | Sends text to `event.chat_id`; see the caveat below. |

### Reply Helper Caveat

The event helper is bound as `event.reply(text[, delay_sec])`, not as a Lua method that accepts an implicit `self`. Use dot syntax:

```lua
event.reply("Hello", 0)
```

Do not use colon syntax:

```lua
-- Incorrect with the current API: event is passed as an extra argument.
event:reply("Hello", 0)
```

For the clearest and most portable scripts, use:

```lua
ox.send_message(event.chat_id, "Hello", 0)
```

## Errors And Concurrency

- A syntax error or registration error prevents the script from starting.
- A runtime error inside a callback is written to the OxideGram log with the chat and sender IDs.
- Sending functions are asynchronous from the Rust runtime but are called normally from Lua.
- Handlers are invoked sequentially for each received message in the current implementation.
- Long delays or expensive work in one handler can postpone later handlers and updates. Keep handlers short.

Logs are available under `data/logs/`. Set `OXIDEGRAM_LOG=debug` before starting OxideGram for verbose diagnostics.
