---
layout: default
title: Lua API Reference
---

# Lua API Reference

[Documentation home](index.html) | [Filters](filters.html) | [Examples](examples.html)

## Global `ox` Table

The main OxideGram automation API table.

### Registering Handlers

#### `ox.on_message([filters, ]callback)`

Registers a message listener. If a filter table is provided, the callback only executes when all conditions match:

```lua
ox.on_message({
    incoming = true,
    private = true,
    commands = "start"
}, function(event)
    event.reply("Welcome to OxideGram! 🚀", { parse_mode = "markdown" })
end)
```

---

### Sending & Editing Messages

#### Message Options Table

Methods taking an optional options argument accept either:
- A `number`: interpreted as a delay in seconds (e.g. `1.5`).
- A `table`: with fine-grained formatting and delay options:
  - `delay`: number of seconds to wait before execution (with human-like jitter).
  - `parse_mode`: `"markdown"` (or `"md"`) or `"html"` for styled text.

#### `ox.send_message(chat_id, text[, options_or_delay[, extra_delay]])`

Sends a text message to the specified chat ID. Automatically handles `FloodWait` by backing off and retrying.

```lua
ox.send_message(event.chat_id, "Hello world!")
ox.send_message(event.chat_id, "*Bold text*", { parse_mode = "markdown", delay = 1.0 })
```

#### `ox.edit_message(chat_id, message_id, new_text[, options])`

Edits an existing message sent by your userbot.

```lua
ox.edit_message(event.chat_id, event.message_id, "Updated text", { parse_mode = "markdown" })
```

#### `ox.delete_message(chat_id, message_id[, delay])`

Deletes a single message by ID.

```lua
ox.delete_message(event.chat_id, event.message_id, 2.0)
```

#### `ox.delete_messages(chat_id, message_ids[, delay])`

Deletes multiple messages in a chat.

```lua
ox.delete_messages(event.chat_id, { 101, 102, 103 })
```

#### `ox.send_reaction(chat_id, message_id, emoji[, delay])` / `ox.react`

Sends an emoji reaction to a message.

```lua
ox.send_reaction(event.chat_id, event.message_id, "👍")
ox.react(event.chat_id, event.message_id, "🔥", 0.5)
```

#### `ox.pin_message(chat_id, message_id[, delay])`

Pins a message in the specified chat.

```lua
ox.pin_message(event.chat_id, event.message_id)
```

#### `ox.forward_message(to_chat_id, from_chat_id, message_id[, delay])`

Forwards a message from one dialog to another.

```lua
ox.forward_message(dest_chat_id, event.chat_id, event.message_id)
```

---

### Media Operations

#### `ox.send_image(chat_id, path[, caption[, options]])`

Uploads and sends a local photo.

```lua
ox.send_image(event.chat_id, "data/banner.png", "Look at this!", { parse_mode = "markdown" })
```

#### `ox.send_document(chat_id, path[, caption[, options]])`

Uploads and sends an uncompressed file or document.

```lua
ox.send_document(event.chat_id, "data/report.pdf", "Monthly report")
```

#### `ox.send_audio(chat_id, path[, caption[, options]])`

Uploads and sends an audio track.

```lua
ox.send_audio(event.chat_id, "data/soundtrack.mp3", "Theme song")
```

#### `ox.send_voice(chat_id, path[, delay])`

Uploads an `.ogg` file and sends it as a Telegram voice note.

```lua
ox.send_voice(event.chat_id, "data/voice.ogg", 0.5)
```

---

### Interactive Buttons & Clicks

#### `ox.click_button(chat_id, message_id, query_or_index[, delay])`

Simulates clicking a button on a message from another bot.
- `query_or_index`: button text query (string) or 1-based flat index (integer).
- Automatically executes the MTProto `messages.GetBotCallbackAnswer` RPC for inline callback buttons.

```lua
ox.click_button(event.chat_id, event.message_id, "Verify")
ox.click_button(event.chat_id, event.message_id, 1) -- clicks first button
```

---

### Timers & Background Execution

Timers run non-blockingly via the runner queue without freezing message processing.

#### `ox.set_interval(seconds, callback) -> integer`

Schedules a recurring background callback every `seconds`. Returns a timer ID.

```lua
local timer_id = ox.set_interval(30.0, function()
    ox.log.info("30-second heartbeat check")
end)
```

#### `ox.set_timeout(seconds, callback) -> integer`

Schedules a one-shot background callback after `seconds`.

```lua
ox.set_timeout(5.0, function()
    ox.send_message(my_chat, "Delayed reminder!")
end)
```

#### `ox.clear_timer(timer_id) -> boolean`

Cancels an active interval or timeout.

```lua
ox.clear_timer(timer_id)
```

#### `ox.sleep(seconds)`

Asynchronously pauses the current handler without blocking the background update receiver.

```lua
ox.sleep(2.0)
```

---

### Humanization & Anti-Ban

#### `ox.send_typing(chat_id)`

Sends a Telegram typing action status to the chat, making bot behavior appear human.

```lua
ox.send_typing(event.chat_id)
ox.sleep(1.2)
event.reply("Finished typing response!")
```

---

### Built-in Logging

Writes structured log messages to the OxideGram `tracing` system and rotable log files (`data/logs/`):

- `ox.log.info(message)`
- `ox.log.warn(message)`
- `ox.log.error(message)`
- `ox.log.debug(message)`

```lua
ox.log.info("Bot loaded successfully")
ox.log.warn("Rate limit approached")
```

---

### Process Control & Configuration

#### `ox.stop()`

Gracefully shuts down the bot's event loop and update stream.

#### `ox.input(prompt[, default]) -> string`

Displays an interactive styled prompt in the terminal during startup configuration.

---

## The `event` Object

Every message callback receives an enriched `event` table with metadata and context-bound helper methods.

### Properties

| Field | Type | Description |
| --- | --- | --- |
| `event.text` | string | Message text content (or empty string). |
| `event.id` / `event.message_id` | integer | Numeric identifier of the message. |
| `event.chat_id` | integer | Numeric peer ID of the chat/dialog. |
| `event.sender_id` | integer | Numeric peer ID of the sender (`0` if anonymous). |
| `event.incoming` | boolean | `true` if received from someone else. |
| `event.outgoing` | boolean | `true` if sent from your userbot account. |
| `event.is_private` | boolean | `true` in 1-on-1 private chats. |
| `event.is_group` | boolean | `true` in basic group chats. |
| `event.is_channel` | boolean | `true` in channels and supergroups. |
| `event.matches` | table | 1-based array of positional regex capture groups (`matches[1]`, `matches[2]`). |
| `event.captures` | table | Key-value map of named regex captures (`captures.name`). |
| `event.buttons` | table | 2D array of parsed keyboard rows with button objects. |

### Button Object in `event.buttons`

Each element in `event.buttons[row][col]` contains:
- `btn.text`: button title.
- `btn.type`: `"callback"`, `"url"`, `"text"`, or `"other"`.
- `btn.data`: binary callback payload (string).
- `btn.url`: destination web link (for URL buttons).

### Context Methods on `event`

#### `event.reply(text[, options])`

Replies directly to the received message with a quote (`reply_to`).

```lua
event.reply("Roger that!", { parse_mode = "markdown", delay = 0.5 })
```

#### `event.edit(new_text[, options])`

Edits the received message.

```lua
event.edit("New content")
```

#### `event.delete([delay])`

Deletes the received message.

```lua
event.delete(1.0)
```

#### `event.react(emoji[, delay])`

Reacts to the received message.

```lua
event.react("🎉")
```

#### `event.pin([delay])`

Pins the received message in the chat.

```lua
event.pin()
```

#### `event.click(query_or_index[, delay])` / `event.click_button`

Clicks an inline callback button or simulates pressing a reply keyboard button on this message.

```lua
-- Find button by text label and click it
event.click("Confirm")

-- Click by 1-based index (e.g. 1 = first button in keyboard)
event.click(1)
```
