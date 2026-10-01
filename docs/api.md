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

### Standard Library & String Extensions

All Lua strings have extended metatable methods for easy pattern-free text operations:

- `str:contains(substring) -> boolean`: Checks if substring is present.
- `str:starts_with(prefix) -> boolean`: Checks if string starts with prefix.
- `str:ends_with(suffix) -> boolean`: Checks if string ends with suffix.
- `str:split([delimiter]) -> table`: Splits string into an array (default delimiter: `" "`).
- `str:trim() -> string`: Trims whitespace from both ends.
- `str:to_lower() -> string`: Converts to lowercase.
- `str:to_upper() -> string`: Converts to uppercase.

These are also available as global functions: `contains(str, sub)`, `starts_with(str, prefix)`, `ends_with(str, suffix)`, `split(str, sep)`, `trim(str)`.

```lua
if event.text:contains("💋 или 👋") then
    event.click("💌 Сообщение")
end
```

#### Random & Delay Helpers

- `ox.sleep_random(min_seconds, max_seconds)`: Asynchronously pauses execution for a random duration with humanized timing.
- `ox.choice(array_table) -> any`: Returns a uniformly random element from a Lua array.
- `ox.random_int(min, max) -> integer`: Returns a random integer in `[min, max]`.

```lua
local greetings = { "Hello!", "Hi there!", "Hey!" }
local msg = ox.choice(greetings)
ox.sleep_random(1.0, 3.5)
event.reply(msg)
```

---

### Persistent Storage (`ox.storage`)

Key-value JSON storage saved to `data/storage/<bot_name>.json`. Automatically reloaded on bot restart.

- `ox.storage.get(key[, default]) -> any`: Retrieves a stored value or default if not found.
- `ox.storage.set(key, value)`: Stores a value (string, number, boolean, or table).
- `ox.storage.has(key) -> boolean`: Checks if a key exists in storage.
- `ox.storage.delete(key) -> boolean`: Removes a key.
- `ox.storage.all() -> table`: Returns all stored key-value pairs as a table.
- `ox.storage.clear()`: Deletes all stored data for this bot.

```lua
local counter = ox.storage.get("processed_users", 0) + 1
ox.storage.set("processed_users", counter)
ox.log.info("Total users processed: " .. counter)
```

---

### Dialog Flow & FSM (`ox.flow`)

Build declarative, step-by-step state machines for complex multi-turn dialogs.

#### `ox.flow(name[, options]) -> Flow`

Options table:
- `target_chat` (integer, optional): Restricts the flow to a specific chat ID.
- `timeout` (number, optional): Max seconds before current step expires and resets.

#### Flow Methods

- `flow:step(name, config)`: Defines a named step with matching criteria and handler action:
  - `config.match`: String or table of strings to match incoming message text.
  - `config.commands`: Command or table of commands (e.g. `{"start"}`).
  - `config.pattern`: Regex pattern.
  - `config.action(event, ctx)`: Handler function. Can return the name of the next step (string) to transition, or `nil` to stay in current step.
- `flow:go_to(step_name)`: Manually switch to another step.
- `flow:reset()`: Reset flow to initial/idle state.
- `flow:on_match(pattern_or_table, action)`: Global trigger matching regardless of current step (e.g. stop keywords).
- `flow.data`: Shared context table (`ctx.data`) preserved across step transitions.

```lua
local dialog = ox.flow("rating_bot", { timeout = 60 })
dialog.data.score = 5

dialog:step("menu", {
    match = "Меню:",
    action = function(event, ctx)
        event.click("💋 или 👋")
        return "rate"
    end
})

dialog:step("rate", {
    match = "💋 или 👋",
    action = function(event, ctx)
        event.reply("Rating: " .. ctx.data.score)
        return "menu"
    end
})
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

---

## Offline Dry-Run Simulator

OxideGram includes a built-in sandbox simulator to test bot scripts locally without authorizing or connecting to Telegram:

```bash
# Launch interactive simulator for a bot
cargo run -- test-bot data/bots/my_bot.lua
# Or shorthand
cargo run -- sim
```

### Simulator Features
- **Hot-Reload**: Automatically re-runs and reloads scripts upon saving changes on disk.
- **Mock Actions**: `send_message`, `reply`, `edit`, `delete`, `react`, and `click` print visual feedback instead of contacting Telegram.
- **Button Simulation**: Displays simulated inline button keyboards and allows triggering them with `/click`.
- **REPL Commands**:
  - `/click <label_or_index>`: Simulate clicking a button by text or number.
  - `/buttons`: Show the currently active buttons on the last message.
  - `/chat <id>`: Switch current target chat ID.
  - `/state`: Inspect active handlers and persistent `ox.storage` state.
  - `/help`: Show command cheat sheet.
  - `/exit` or `exit`: Quit the simulator.

