---
layout: default
title: API Reference
---

# API Reference

[Home](index.html) | [Filters](filters.html) | [Examples](examples.html)

The OxideGram automation engine provides two primary abstractions for script authors:
- **`ox` global table**: Global functions for message dispatching, client operations, media transfers, timing, persistent storage, and state machines.
- **`event` context object**: Contextual message representations passed directly to message listener callbacks and dialog step handlers.

> [!NOTE]
> All methods on `Flow`, `FlowContext`, `event`, `ox.storage`, and `ox.log` support both method call syntax with a colon (`object:method(...)`) and standard function call syntax (`object.method(...)`).

---

## Global Table `ox`

### Event Handlers

#### `ox.on_message([filters, ]callback)`

Registers an incoming or outgoing message listener. If a filter table is provided, the callback only executes when every specified filter condition is met.

```lua
-- Handler with explicit filter criteria
ox.on_message({
    incoming = true,
    private = true,
    commands = "start",
}, function(event)
    event.reply("Welcome to OxideGram! 🚀", { parse_mode = "markdown" })
end)

-- Catch-all handler without filters
ox.on_message(function(event)
    ox.log.info(string.format("Message from chat %d: %s", event.chat_id, event.text))
end)
```

---

### Message Operations

#### Message Options

Functions accepting an optional `options` argument accept either:
- A `number`: interpreted as an execution delay in seconds (e.g. `1.5`).
- A `table`: with fine-grained formatting and timing options:
  - `delay` (*number*, optional): Delay in seconds before executing the request (with automatic human-like jitter).
  - `parse_mode` (*string*, optional): Text formatting parser: `"markdown"` (or `"md"`) or `"html"`.

#### `ox.send_message(chat_id, text[, options_or_delay[, extra_delay]])`

Sends a text message to the specified chat ID. Automatically handles Telegram `FloodWait` by backing off and retrying transparently.

```lua
-- Simple message without delay
ox.send_message(event.chat_id, "Hello from OxideGram!")

-- Formatted Markdown message with a 1.0 second delay
ox.send_message(event.chat_id, "*Bold title*\n_Italic description_", {
    parse_mode = "markdown",
    delay = 1.0,
})

-- Formatted HTML message
ox.send_message(event.chat_id, "<b>Alert:</b> <code>System online</code>", {
    parse_mode = "html",
    delay = 0.5,
})
```

#### `ox.edit_message(chat_id, message_id, new_text[, options])`

Edits the text of an existing message previously sent by the userbot account.

```lua
local status = ox.send_message(event.chat_id, "Processing request...")
ox.sleep(1.5)
ox.edit_message(event.chat_id, event.message_id, "Processing complete! Result: *Success*", {
    parse_mode = "markdown",
})
```

#### `ox.delete_message(chat_id, message_id[, delay])`

Deletes a single message by ID.

```lua
-- Delete immediately
ox.delete_message(event.chat_id, event.message_id)

-- Delete after a 5-second countdown
ox.delete_message(event.chat_id, event.message_id, 5.0)
```

#### `ox.delete_messages(chat_id, message_ids[, delay])`

Deletes multiple messages simultaneously within a dialog.

```lua
local ids_to_purge = { 101, 102, 103, 104 }
ox.delete_messages(event.chat_id, ids_to_purge, 1.0)
```

#### `ox.send_reaction(chat_id, message_id, emoji[, delay])` (Alias: `ox.react`)

Applies an emoji reaction to a specified message.

```lua
ox.send_reaction(event.chat_id, event.message_id, "👍")
ox.react(event.chat_id, event.message_id, "🔥", 0.5)
```

#### `ox.pin_message(chat_id, message_id[, delay])`

Pins a message in the specified chat.

```lua
ox.pin_message(event.chat_id, event.message_id)
ox.pin_message(event.chat_id, event.message_id, 1.0)
```

#### `ox.forward_message(to_chat_id, from_chat_id, message_id[, delay])`

Forwards an existing message from one chat into another.

```lua
local archive_channel = -1001234567890
ox.forward_message(archive_channel, event.chat_id, event.message_id, 0.5)
```

---

### Media Operations

#### `ox.send_image(chat_id, path[, caption[, options]])`

Uploads and sends an image file from the local filesystem (`.png`, `.jpg`, `.jpeg`, `.webp`).

```lua
ox.send_image(event.chat_id, "data/banner.png", "System Status Dashboard", {
    parse_mode = "markdown",
    delay = 1.0,
})
```

#### `ox.send_document(chat_id, path[, caption[, options]])`

Uploads and sends an uncompressed file or document (`.pdf`, `.zip`, `.csv`, `.txt`).

```lua
ox.send_document(event.chat_id, "data/export.csv", "Exported records", {
    delay = 0.5,
})
```

#### `ox.send_audio(chat_id, path[, caption[, options]])`

Uploads and sends an audio track (`.mp3`, `.m4a`, `.flac`).

```lua
ox.send_audio(event.chat_id, "data/soundtrack.mp3", "Theme Song", {
    parse_mode = "markdown",
})
```

#### `ox.send_voice(chat_id, path[, delay])`

Uploads an `.ogg` file and transmits it as a native playable Telegram voice message.

```lua
ox.send_voice(event.chat_id, "data/greeting.ogg", 1.0)
```

---

### Keyboard Buttons

#### `ox.click_button(chat_id, message_id, query_or_index[, delay])`

Programmatically simulates clicking a button on an inline or reply keyboard attached to a message.

- `query_or_index`: Exact or fuzzy button text (string) or 1-based sequential index (integer).
- Automatically executes the MTProto `GetBotCallbackAnswer` procedure for inline callback buttons.
- For reply keyboard buttons, automatically locates the matching button title and sends the response message.

```lua
-- Click button by label
ox.click_button(event.chat_id, event.message_id, "Verify")

-- Click the first button in the keyboard with a 1.2 second delay
ox.click_button(event.chat_id, event.message_id, 1, 1.2)
```

---

### Timing & Delays

Timers execute non-blockingly inside the asynchronous runtime without stalling message reception.

#### `ox.sleep(seconds)`

Asynchronously pauses execution in the current callback without blocking background update polling or other concurrent scripts.

```lua
ox.sleep(2.5)
```

#### `ox.sleep_random(min_seconds, max_seconds)`

Asynchronously pauses execution for a randomized duration between `min_seconds` and `max_seconds` with natural timing jitter.

```lua
-- Wait between 1.0 and 3.5 seconds
ox.sleep_random(1.0, 3.5)
```

#### `ox.set_timeout(seconds, callback) -> integer`

Schedules a one-time callback after a specified duration in seconds. Returns a unique integer timer identifier.

```lua
local timer_id = ox.set_timeout(10.0, function()
    ox.log.info("10-second delayed task executed")
end)
```

#### `ox.set_interval(seconds, callback) -> integer`

Schedules a recurring background callback executed repeatedly every `seconds`. Returns a unique integer timer identifier.

```lua
local ping_timer = ox.set_interval(60.0, function()
    ox.log.info("Heartbeat tick: connection alive")
end)
```

#### `ox.clear_timer(timer_id) -> boolean`

Cancels an active interval or timeout by its ID. Returns `true` if the timer was found and cancelled.

```lua
ox.clear_timer(ping_timer)
```

---

### Chat Actions

#### `ox.send_typing(chat_id)`

Sends an ephemeral typing indicator to the specified chat to mimic human behavior before sending a response.

```lua
ox.send_typing(event.chat_id)
ox.sleep_random(1.0, 2.5)
event.reply("Generated response text.")
```

---

### Logging

Writes structured log messages to stdout, the terminal UI, and rotating log files (`data/logs/`):

- `ox.log.info(message)`
- `ox.log.warn(message)`
- `ox.log.error(message)`
- `ox.log.debug(message)`

```lua
ox.log.info("Bot loaded successfully")
ox.log.warn(string.format("Rate limit warning for chat %d", event.chat_id))
ox.log.error("Failed to parse external payload")
ox.log.debug("Internal cache state updated")

-- Also callable with colon syntax
ox.log:info("Service operational")
```

---

### Interactive Prompts

Interactive prompts allow userbot scripts to request runtime parameters directly in the terminal before connecting to Telegram.

#### `ox.select(prompt, options[, default]) -> string`

Renders an interactive selection menu with arrow key navigation, fuzzy filtering, and default selection cursor.

`options` can be specified as:
1. An array of strings: `{"text", "grades"}`
2. An array of objects with distinct labels and return values: `{{ label = "...", value = "..." }}`

```lua
-- 1. Simple array of options
local mode = ox.select("Select spam mode:", { "text", "grades" }, "text")

-- 2. Labeled options with explicit underlying values
local target = ox.select("Choose destination:", {
    { label = "General Chat (@general)", value = "@general" },
    { label = "VIP Channel (@vip)", value = "@vip" },
    { label = "Testing Sandbox (@sandbox)", value = "@sandbox" },
}, "@general")
```

#### `ox.confirm(prompt[, default]) -> boolean`

Prompts the user for a boolean yes/no confirmation in the terminal.

```lua
local delete_after = ox.confirm("Delete messages after sending?", true)
if delete_after then
    ox.log.info("Auto-delete mode enabled")
end
```

#### `ox.input(prompt[, default_or_config]) -> string`

Displays a text input prompt. If passed an options configuration table, functions as an interactive select menu.

```lua
-- Simple text prompt with default fallback
local target_channel = ox.input("Target channel username:", "@my_channel")

-- Config table format
local action = ox.input("Select startup action:", {
    options = { "monitor", "relay", "audit" },
    default = "monitor",
})
```

---

### Persistent Storage (`ox.storage`)

Persistent key-value JSON storage backed by disk (`data/storage/<bot_name>.json`). Data survives bot restarts, script reloads, and application crashes.

Methods support both `ox.storage.method(...)` and `ox.storage:method(...)`:

- `ox.storage.get(key[, default]) -> any`: Retrieves a stored value or returns the default fallback.
- `ox.storage.set(key, value)`: Stores a value (string, number, boolean, or nested table).
- `ox.storage.has(key) -> boolean`: Checks if a key exists in storage.
- `ox.storage.delete(key) -> boolean`: Deletes a key from storage.
- `ox.storage.all() -> table`: Returns all stored key-value pairs as a Lua table.
- `ox.storage.clear()`: Purges all stored data for this script.

```lua
-- Counter persistence example
local count = ox.storage.get("processed_messages", 0) + 1
ox.storage.set("processed_messages", count)
ox.log.info("Messages processed so far: " .. count)

-- Complex nested tables
ox.storage.set("user_preferences", {
    theme = "dark",
    auto_reply = true,
    keywords = { "help", "support", "pricing" },
})

local prefs = ox.storage.get("user_preferences")
if prefs and prefs.auto_reply then
    ox.log.info("Auto-reply preference is active")
end

-- Inspecting all keys
for key, val in pairs(ox.storage.all()) do
    ox.log.debug(string.format("Storage [%s] = %s", key, tostring(val)))
end
```

---

### State Machine (`ox.flow`)

The Dialog Flow engine enables declarative, multi-step finite state machines for complex multi-turn dialogs, questionnaires, and scenarios.

#### `ox.flow(name[, options]) -> Flow`

Constructs and registers a new state machine.

Options table:
- `target_chat` (*integer*, optional): Restricts the flow to a specific chat ID.
- `target_chats` (*integer[]*, optional): Restricts the flow to a list of chat IDs.
- `timeout` (*number*, optional): Maximum idle duration in seconds before the active step expires.
- `initial` (*string*, optional): Explicit name of the starting step. If omitted, the first step registered via `flow:step` becomes initial.

#### `Flow:step(name, config)`

Defines a step in the state machine.

Configuration table:
- `match` (*string* or *string[]*, optional): Substrings to match in incoming message text.
- `commands` (*string* or *string[]*, optional): Slash commands that activate this step (e.g. `"start"` or `{"start", "menu"}`).
- `pattern` (*string*, optional): Regular expression pattern that activates this step.
- `timeout` (*number*, optional): Per-step timeout override in seconds.
- `on_timeout` (*function(ctx)*, optional): Callback executed when the step times out.
- `next` (*string*, optional): Default next step name when `action` does not explicitly return one.
- `action` (*function(event, ctx)*, required): Handler function executed when the step criteria match. Returning a step name transitions to that step. Returning `nil` remains in the current step.

#### Step Context Object (`ctx`)
- `ctx.step` (*string*): Name of the current step.
- `ctx.data` (*table*): Shared context table persisted across all step transitions in this dialog.
- `ctx:go_to(step_name)`: Programmatically transitions to another step.
- `ctx:reset()`: Resets the flow to its initial step.

#### `Flow:on_match(pattern, action)`

Registers a global pattern or keyword trigger that executes regardless of the currently active step (e.g. cancellation commands).

#### `Flow:go_to(step_name[, chat_id])`

Manually transitions the state machine to a specific step.

#### `Flow:reset([chat_id])`

Resets the state machine for the specified chat (or all chats) to its initial step.

#### `Flow:current_step([chat_id]) -> string?`

Returns the name of the currently active step.

```lua
local dialog = ox.flow("onboarding_flow", {
    timeout = 45, -- 45-second timeout per step
})

dialog.data.answers = {}

-- 1. Initial Step
dialog:step("ask_name", {
    match = "start",
    commands = "start",
    action = function(event, ctx)
        event.reply("Hello! What is your name?")
        return "await_name"
    end,
})

-- 2. Collect Name Step
dialog:step("await_name", {
    action = function(event, ctx)
        ctx.data.answers.name = event.text:trim()
        event.reply(string.format("Nice to meet you, %s! Are you ready to proceed?", ctx.data.answers.name))
        return "confirm"
    end,
    on_timeout = function(ctx)
        ox.log.warn("User took too long to provide a name. Resetting dialog.")
    end,
})

-- 3. Confirmation Step
dialog:step("confirm", {
    match = { "да", "yes", "ready" },
    action = function(event, ctx)
        event.reply("Setup complete! Your account is configured.")
        return "ask_name" -- loop back or end
    end,
})

-- Global abort trigger
dialog:on_match({ "cancel", "abort", "стоп" }, function(event)
    dialog:reset(event.chat_id)
    event.reply("Operation cancelled. Dialog reset.")
end)
```

---

### Process Control

#### `ox.stop()`

Gracefully shuts down the bot's event loop and update stream.

```lua
ox.on_message({ commands = "shutdown", outgoing = true }, function(event)
    event.reply("Shutting down OxideGram...")
    ox.sleep(1.0)
    ox.stop()
end)
```

---

### Utility Helpers

#### String Extensions

All Lua strings have metatable methods attached for pattern-free text operations:

- `str:contains(substring[, case_insensitive]) -> boolean`: Checks if substring is present (default case-insensitive: `true`).
- `str:starts_with(prefix[, case_insensitive]) -> boolean`: Checks if string starts with prefix.
- `str:ends_with(suffix[, case_insensitive]) -> boolean`: Checks if string ends with suffix.
- `str:split([delimiter]) -> table`: Splits string into an array (default delimiter: `" "`).
- `str:trim() -> string`: Trims whitespace from both ends.
- `str:to_lower() -> string`: Converts string to lowercase.
- `str:to_upper() -> string`: Converts string to uppercase.

These methods are also exposed as global functions: `contains(s, sub)`, `starts_with(s, prefix)`, `ends_with(s, suffix)`, `split(s, sep)`, and `trim(s)`.

```lua
local query = "  /search RUST ENGINE  "
local clean = query:trim()               -- "/search RUST ENGINE"
local parts = clean:split(" ")           -- { "/search", "RUST", "ENGINE" }

if clean:starts_with("/search") and clean:contains("rust") then
    ox.log.info("Search query matched: " .. parts[2])
end
```

#### `ox.choice(array_table) -> any`

Returns a uniformly distributed random item from a sequential array table.

```lua
local greetings = { "Hello!", "Hi there!", "Welcome aboard!", "Hey!" }
local picked = ox.choice(greetings)
```

#### `ox.random_int(min, max) -> integer`

Generates a secure pseudo-random integer in the closed interval `[min, max]`.

```lua
local delay_ms = ox.random_int(200, 800)
local roll = ox.random_int(1, 6)
```

---

## Message Event Object (`event`)

Every message handler callback receives an enriched `event` table.

### Event Properties

| Field | Type | Description |
| :--- | :--- | :--- |
| `event.text` | string | Message text content (or empty string if non-text media). |
| `event.id` / `event.message_id` | integer | Unique message identifier. |
| `event.chat_id` | integer | Signed 64-bit peer identifier of the chat or dialog. |
| `event.sender_id` | integer | Signed 64-bit peer identifier of the sender (`0` if anonymous channel message). |
| `event.incoming` | boolean | `true` if the message was sent by another account. |
| `event.outgoing` | boolean | `true` if the message was sent by your account. |
| `event.is_private` | boolean | `true` for 1-on-1 private chats with users. |
| `event.is_group` | boolean | `true` for basic group chats. |
| `event.is_channel` | boolean | `true` for broadcast channels and supergroups. |
| `event.matches` | string[] | Positional regex captures (`matches[1]`, `matches[2]`). |
| `event.captures` | table | Named regex captures (`captures.id`, `captures.user`). |
| `event.buttons` | Button[][] | 2D matrix of keyboard buttons attached to this message. |

### Button Representation (`event.buttons`)

Each button in `event.buttons[row][col]` contains:
- `btn.text` (*string*): Visible text label on the button.
- `btn.type` (*string*): Button kind: `"callback"`, `"url"`, `"text"`, or `"other"`.
- `btn.data` (*string*): Binary callback payload data (for inline buttons).
- `btn.url` (*string*): Target hyperlink destination (for URL buttons).

```lua
ox.on_message({ incoming = true }, function(event)
    if #event.buttons > 0 then
        ox.log.info(string.format("Detected %d button rows", #event.buttons))
        for row_idx, row in ipairs(event.buttons) do
            for col_idx, btn in ipairs(row) do
                ox.log.debug(string.format("Btn [%d,%d]: '%s' (%s)", row_idx, col_idx, btn.text, btn.type))
            end
        end
    end
end)
```

### Event Methods

Methods on `event` support both `event:method(...)` and `event.method(...)`:

#### `event:reply(text[, options])`

Replies directly to the received message with a quote (`reply_to`).

```lua
event:reply("Acknowledged!", { parse_mode = "markdown", delay = 0.5 })
```

#### `event:edit(new_text[, options])`

Edits the text of the received message.

```lua
event:edit("Updated content", { parse_mode = "markdown" })
```

#### `event:delete([delay])`

Deletes the received message.

```lua
event:delete(2.0)
```

#### `event:react(emoji[, delay])`

Applies an emoji reaction directly to this message.

```lua
event:react("🔥", 0.5)
```

#### `event:pin([delay])`

Pins this message in the current dialog.

```lua
event:pin()
```

#### `event:click(query_or_index[, delay])` (Alias: `event:click_button`)

Clicks an inline callback button or simulates pressing a regular reply keyboard button attached to this message.

```lua
-- Click by label (Inline or Reply keyboard)
event:click("Verify")

-- Click by 1-based flat index
event:click(1, 1.0)
```

---

## Offline Simulator

OxideGram includes a built-in interactive simulator to test bot scripts locally without connecting to Telegram:

```bash
# Launch interactive simulator for a bot
oxidegram sim hello

# Or via cargo
cargo run -- sim hello
```

### Simulator Commands

| Command | Description |
| :--- | :--- |
| `/click <query_or_index>` | Simulates clicking an inline or reply keyboard button. |
| `/buttons` | Displays all keyboard buttons attached to the most recent message. |
| `/chat <id>` | Switches the current simulated chat ID (e.g. `/chat 123456789`). |
| `/state` | Displays active message listeners and current `ox.storage` contents. |
| `/help` | Prints simulator command reference. |
| `/exit` | Exits the simulator REPL. |
