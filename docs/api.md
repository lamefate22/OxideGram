---
layout: default
title: API Reference
---

# API Reference

[Home](index.html) | [Filters](filters.html) | [Cookbook & Examples](examples.html)

The OxideGram scripting engine provides two core programming interfaces:
- **`ox` global table**: Functions for message transmission, media handling, interactive console prompts, persistent storage, scheduling, string manipulation, and state machines.
- **`event` context object**: Enriched message representation passed to message listeners and dialog flow action handlers.

> [!NOTE]
> All object methods on `event`, `flow`, `ctx`, `ox.storage`, and `ox.log` support both colon syntax (`object:method(...)`) and dot syntax (`object.method(...)`).
> Most functions support both sequential positional arguments and single-table named argument syntax (e.g. `ox.send_message { chat_id = ..., text = ... }`).

---

## Event Handling

### `ox.on_message`

Registers a message event listener. The callback executes whenever an incoming or outgoing message satisfies the optional filter criteria.

#### Syntax

```lua
ox.on_message([filter, ]callback)
```

#### Parameters

| Parameter | Type | Required | Description | Default |
|:---|:---|:---|:---|:---|
| `filter` | `table` | No | Predicate table restricting which messages trigger the handler. | `nil` (matches all messages) |
| `callback` | `function(event)` | Yes | Asynchronous function executed when a message matches. | — |

#### Examples

```lua
-- Example 1: Catch-all listener logging every message
ox.on_message(function(event)
    ox.log.info(string.format("Chat %d: %s", event.chat_id, event.text))
end)

-- Example 2: Filter by chat type and slash command
ox.on_message({
    incoming = true,
    private = true,
    commands = "start",
}, function(event)
    event:reply("Welcome to OxideGram!")
end)

-- Example 3: Filter by regex pattern with capture groups
ox.on_message({
    incoming = true,
    pattern = "^/calc\\s+(?P<expr>.+)$",
}, function(event)
    local expr = event.captures["expr"]
    event:reply("Expression: " .. expr)
end)
```

---

## Messaging

### `ox.send_message`

Sends a text message to a Telegram dialog or user. Automatically handles rate-limiting (`FloodWait`) with exponential backoff.

#### Syntax

```lua
ox.send_message(chat_id, text[, options])
ox.send_message(options_table)
```

#### Parameters

| Parameter | Type | Required | Description | Default |
|:---|:---|:---|:---|:---|
| `chat_id` | `integer` | Yes | Target Telegram peer / chat ID. | — |
| `text` | `string` | Yes | Text content of the message. | — |
| `options` | `table` \| `number` | No | Configuration table or numeric delay in seconds. | `nil` |
| `options.delay` | `number` | No | Delay in seconds before sending (with natural jitter). | `0` |
| `options.parse_mode` | `string` | No | Text parser: `"markdown"` (or `"md"`) or `"html"`. | `nil` (plain text) |

#### Examples

```lua
-- Example 1: Basic message
ox.send_message(123456789, "Hello from OxideGram!")

-- Example 2: Markdown formatting with delay
ox.send_message(123456789, "*System Notification*\nTask `sync` completed.", {
    parse_mode = "markdown",
    delay = 1.0,
})

-- Example 3: HTML formatting
ox.send_message(123456789, "<b>Status:</b> <code>Online</code>", {
    parse_mode = "html",
})

-- Example 4: Named table syntax
ox.send_message {
    chat_id = 123456789,
    text = "Report ready for download.",
    delay = 0.5,
}
```

---

### `ox.edit_message`

Edits the text and formatting of an existing message sent by your userbot.

#### Syntax

```lua
ox.edit_message(chat_id, message_id, new_text[, options])
ox.edit_message(options_table)
```

#### Parameters

| Parameter | Type | Required | Description | Default |
|:---|:---|:---|:---|:---|
| `chat_id` | `integer` | Yes | Peer identifier of the chat containing the message. | — |
| `message_id` | `integer` | Yes | Numeric ID of the message to edit. | — |
| `new_text` | `string` | Yes | New message text. | — |
| `options` | `table` \| `number` | No | Configuration table or numeric delay in seconds. | `nil` |
| `options.delay` | `number` | No | Delay in seconds before editing. | `0` |
| `options.parse_mode` | `string` | No | Text parser: `"markdown"` or `"html"`. | `nil` |

#### Examples

```lua
-- Example 1: Basic edit
ox.edit_message(event.chat_id, event.message_id, "Processing completed.")

-- Example 2: Edit with Markdown formatting and delay
ox.edit_message(event.chat_id, event.message_id, "*Updated Status:* _Success_", {
    parse_mode = "markdown",
    delay = 0.5,
})

-- Example 3: Named table syntax
ox.edit_message {
    chat_id = event.chat_id,
    message_id = event.message_id,
    text = "<code>Error: Connection timeout</code>",
    parse_mode = "html",
}
```

---

### `ox.delete_message`

Deletes a single message by ID.

#### Syntax

```lua
ox.delete_message(chat_id, message_id_or_ids[, delay])
ox.delete_message(options_table)
```

#### Parameters

| Parameter | Type | Required | Description | Default |
|:---|:---|:---|:---|:---|
| `chat_id` | `integer` | Yes | Peer identifier of the chat. | — |
| `message_id_or_ids` | `integer` \| `integer[]` | Yes | Single message ID or array of numeric message IDs to delete. | — |
| `delay` | `number` | No | Optional delay in seconds before deleting. | `0` |

#### Examples

```lua
-- Example 1: Immediate single message deletion
ox.delete_message(event.chat_id, event.message_id)

-- Example 2: Delete multiple messages at once
ox.delete_message(event.chat_id, { 101, 102, 103 })

-- Example 3: Delayed self-destruct after 5 seconds
ox.delete_message(event.chat_id, event.message_id, 5.0)

-- Example 4: Named table syntax for single message
ox.delete_message {
    chat_id = event.chat_id,
    message_id = event.message_id,
    delay = 3.0,
}

-- Example 5: Named table syntax for multiple messages
ox.delete_message {
    chat_id = event.chat_id,
    ids = { 101, 102, 103 },
    delay = 1.0,
}
```

---

### `ox.react`

Applies an emoji reaction to a specified message.

#### Syntax

```lua
ox.react(chat_id, message_id, emoji[, delay])
ox.react(options_table)
```

#### Parameters

| Parameter | Type | Required | Description | Default |
|:---|:---|:---|:---|:---|
| `chat_id` | `integer` | Yes | Peer identifier of the chat. | — |
| `message_id` | `integer` | Yes | Target message ID. | — |
| `emoji` | `string` | Yes | Reaction emoji (e.g. `"👍"`, `"🔥"`, `"❤️"`). | — |
| `delay` | `number` | No | Delay in seconds before reacting. | `0` |

#### Examples

```lua
-- Example 1: Instant reaction
ox.react(event.chat_id, event.message_id, "👍")

-- Example 2: React with delay
ox.react(event.chat_id, event.message_id, "🔥", 0.5)

-- Example 3: Named table syntax
ox.react {
    chat_id = event.chat_id,
    message_id = event.message_id,
    emoji = "🎉",
    delay = 0.2,
}
```

---

### `ox.pin_message`

Pins a message in the specified chat.

#### Syntax

```lua
ox.pin_message(chat_id, message_id[, delay])
ox.pin_message(options_table)
```

#### Parameters

| Parameter | Type | Required | Description | Default |
|:---|:---|:---|:---|:---|
| `chat_id` | `integer` | Yes | Peer identifier of the chat. | — |
| `message_id` | `integer` | Yes | Message ID to pin. | — |
| `delay` | `number` | No | Delay in seconds before pinning. | `0` |

#### Examples

```lua
-- Example 1: Pin immediately
ox.pin_message(event.chat_id, event.message_id)

-- Example 2: Pin with delay via named table
ox.pin_message {
    chat_id = event.chat_id,
    message_id = event.message_id,
    delay = 1.0,
}
```

---

### `ox.forward_message`

Forwards an existing message from one chat to another.

#### Syntax

```lua
ox.forward_message(to_chat_id, from_chat_id, message_id[, delay])
ox.forward_message(options_table)
```

#### Parameters

| Parameter | Type | Required | Description | Default |
|:---|:---|:---|:---|:---|
| `to_chat_id` | `integer` | Yes | Destination chat ID. | — |
| `from_chat_id` | `integer` | Yes | Source chat ID. | — |
| `message_id` | `integer` | Yes | Message ID to forward. | — |
| `delay` | `number` | No | Delay in seconds before forwarding. | `0` |

#### Examples

```lua
-- Example 1: Forward to archive channel
ox.forward_message(-1001234567890, event.chat_id, event.message_id)

-- Example 2: Named table syntax
ox.forward_message {
    to_chat_id = -1001234567890,
    from_chat_id = event.chat_id,
    message_id = event.message_id,
    delay = 0.5,
}
```

---

### `ox.send_typing`

Broadcasts a temporary chat action status (e.g. typing indicator) to emulate human activity.

#### Syntax

```lua
ox.send_typing(chat_id[, action])
```

#### Parameters

| Parameter | Type | Required | Description | Default |
|:---|:---|:---|:---|:---|
| `chat_id` | `integer` | Yes | Target chat ID. | — |
| `action` | `string` | No | Action type: `"typing"`, `"cancel"`, `"record_video"`, `"upload_video"`, `"record_voice"`, `"upload_voice"`, `"upload_document"`, `"choose_sticker"`. | `"typing"` |

#### Examples

```lua
-- Example 1: Display typing indicator before reply
ox.send_typing(event.chat_id)
ox.sleep_random(1.0, 2.0)
event:reply("Here is your answer!")

-- Example 2: Explicit upload action
ox.send_typing(event.chat_id, "upload_document")
```

---

### `ox.click_button`

Simulates clicking an inline callback button or pressing a regular reply keyboard button.

#### Syntax

```lua
ox.click_button(chat_id, message_id, data)
ox.click_button(options_table)
```

#### Parameters

| Parameter | Type | Required | Description | Default |
|:---|:---|:---|:---|:---|
| `chat_id` | `integer` | Yes | Chat ID containing the button. | — |
| `message_id` | `integer` | Yes | Message ID containing the keyboard. | — |
| `data` | `string` | Yes | Callback data string of the button. | — |

#### Examples

```lua
-- Example 1: Click button by callback payload
ox.click_button(event.chat_id, event.message_id, "agree_terms")

-- Example 2: Named table syntax
ox.click_button {
    chat_id = event.chat_id,
    message_id = event.message_id,
    data = "page_next",
}
```

---

## Media

### `ox.send_image`

Uploads and sends an image file (`.png`, `.jpg`, `.jpeg`, `.webp`) from the local filesystem.

#### Syntax

```lua
ox.send_image(chat_id, path[, caption[, options]])
ox.send_image(options_table)
```

#### Parameters

| Parameter | Type | Required | Description | Default |
|:---|:---|:---|:---|:---|
| `chat_id` | `integer` | Yes | Target chat ID. | — |
| `path` | `string` | Yes | Filesystem path to the image file. | — |
| `caption` | `string` | No | Optional caption text attached to the photo. | `nil` |
| `options` | `table` \| `number` | No | Configuration table or numeric delay in seconds. | `nil` |
| `options.delay` | `number` | No | Delay in seconds before sending. | `0` |
| `options.parse_mode` | `string` | No | Caption formatting parser (`"markdown"` or `"html"`). | `nil` |

#### Examples

```lua
-- Example 1: Send image with caption
ox.send_image(event.chat_id, "data/banner.png", "System Status Dashboard")

-- Example 2: Image with formatted caption and delay
ox.send_image(event.chat_id, "data/chart.png", "*Weekly Report Summary*", {
    parse_mode = "markdown",
    delay = 1.0,
})

-- Example 3: Named table syntax
ox.send_image {
    chat_id = event.chat_id,
    path = "data/photo.jpg",
    caption = "<b>User Avatar</b>",
    parse_mode = "html",
}
```

---

### `ox.send_document`

Uploads and sends a generic file or document (`.pdf`, `.zip`, `.csv`, `.txt`, etc.).

#### Syntax

```lua
ox.send_document(chat_id, path[, caption[, options]])
ox.send_document(options_table)
```

#### Parameters

| Parameter | Type | Required | Description | Default |
|:---|:---|:---|:---|:---|
| `chat_id` | `integer` | Yes | Target chat ID. | — |
| `path` | `string` | Yes | Filesystem path to the document. | — |
| `caption` | `string` | No | Optional caption text. | `nil` |
| `options` | `table` \| `number` | No | Options table or numeric delay in seconds. | `nil` |

#### Examples

```lua
-- Example 1: Send document
ox.send_document(event.chat_id, "data/export.csv", "Exported database records")

-- Example 2: Named table syntax
ox.send_document {
    chat_id = event.chat_id,
    path = "data/archive.zip",
    caption = "Backup archive",
    delay = 1.5,
}
```

---

### `ox.send_audio`

Uploads and sends an audio track (`.mp3`, `.m4a`, `.flac`).

#### Syntax

```lua
ox.send_audio(chat_id, path[, caption[, options]])
ox.send_audio(options_table)
```

#### Parameters

| Parameter | Type | Required | Description | Default |
|:---|:---|:---|:---|:---|
| `chat_id` | `integer` | Yes | Target chat ID. | — |
| `path` | `string` | Yes | Filesystem path to audio file. | — |
| `caption` | `string` | No | Optional caption text. | `nil` |
| `options` | `table` \| `number` | No | Options table or numeric delay. | `nil` |

#### Examples

```lua
ox.send_audio(event.chat_id, "media/podcast.mp3", "Episode 12")

ox.send_audio {
    chat_id = event.chat_id,
    path = "media/song.flac",
    caption = "*Hi-Fi Audio Track*",
    parse_mode = "markdown",
}
```

---

### `ox.send_voice`

Transmits an audio file (`.ogg` with Opus codec) as a native playable Telegram voice message.

#### Syntax

```lua
ox.send_voice(chat_id, path[, caption[, options]])
ox.send_voice(options_table)
```

#### Parameters

| Parameter | Type | Required | Description | Default |
|:---|:---|:---|:---|:---|
| `chat_id` | `integer` | Yes | Target chat ID. | — |
| `path` | `string` | Yes | Filesystem path to voice `.ogg` file. | — |
| `caption` | `string` | No | Optional caption text. | `nil` |
| `options` | `table` \| `number` | No | Options table or delay in seconds. | `nil` |

#### Examples

```lua
ox.send_voice(event.chat_id, "data/greeting.ogg")

ox.send_voice {
    chat_id = event.chat_id,
    path = "data/memo.ogg",
    delay = 1.0,
}
```

---

## Interactive Input

Interactive prompt functions execute during script startup and configuration, prompting the operator via the console before the bot begins processing messages.

### `ox.input`

Prompts the operator for text input in the terminal.

#### Syntax

```lua
ox.input(prompt[, default])
ox.input(options_table)
```

#### Parameters

| Parameter | Type | Required | Description | Default |
|:---|:---|:---|:---|:---|
| `prompt` | `string` | Yes | Text prompt shown to the user. | — |
| `default` | `string` | No | Default fallback value when Enter is pressed without typing. | `nil` |

#### Examples

```lua
-- Example 1: Basic text prompt
local username = ox.input("Enter target username:")

-- Example 2: Text prompt with default fallback
local api_url = ox.input("API Server URL:", "https://api.example.com")

-- Example 3: Named table syntax
local bot_tag = ox.input {
    prompt = "Bot instance tag:",
    default = "prod-1",
}
```

---

### `ox.select`

Renders an interactive terminal selection menu with arrow key navigation, search filtering, and default cursor positioning.

#### Syntax

```lua
ox.select(prompt, options[, default])
ox.select(options_table)
```

#### Parameters

| Parameter | Type | Required | Description | Default |
|:---|:---|:---|:---|:---|
| `prompt` | `string` | Yes | Prompt label displayed in the terminal. | — |
| `options` | `(string \| table)[]` | Yes | Array of strings or tables with `label` and `value`. | — |
| `default` | `string` | No | Default option pre-selected by cursor. | First option |

#### Examples

```lua
-- Example 1: String array
local mode = ox.select("Choose operating mode:", { "fast", "safe", "stealth" }, "safe")

-- Example 2: Key-value objects with labels and underlying values
local target = ox.select("Select destination channel:", {
    { label = "General Chat (@general)", value = "@general" },
    { label = "VIP Group (@vip_members)", value = "@vip_members" },
    { label = "Debug Sandbox (@sandbox)", value = "@sandbox" },
}, "@general")

-- Example 3: Named table syntax
local choice = ox.select {
    prompt = "Desired action:",
    options = { "start", "stop", "restart" },
    default = "start",
}
```

---

### `ox.confirm`

Prompts the operator for a boolean yes/no confirmation in the terminal.

#### Syntax

```lua
ox.confirm(prompt[, default])
ox.confirm(options_table)
```

#### Parameters

| Parameter | Type | Required | Description | Default |
|:---|:---|:---|:---|:---|
| `prompt` | `string` | Yes | Question displayed in the terminal. | — |
| `default` | `boolean` | No | Default value when pressing Enter. | `true` |

#### Examples

```lua
-- Example 1: Confirm with default true (Y/n)
local enable_stats = ox.confirm("Enable metrics reporting?", true)

-- Example 2: Confirm with default false (y/N)
local purge_storage = ox.confirm("Purge local storage database?", false)

-- Example 3: Named table syntax
local proceed = ox.confirm {
    prompt = "Continue execution?",
    default = true,
}
```

---

### `ox.file`

Prompts the operator for a filesystem path with interactive **tab-autocomplete**, path normalization, and automatic validation (existence check and extension filtering).

#### Syntax

```lua
ox.file(prompt[, options_or_default])
ox.file(options_table)
```

#### Parameters

| Parameter | Type | Required | Description | Default |
|:---|:---|:---|:---|:---|
| `prompt` | `string` | Yes | Prompt label displayed in the terminal. | — |
| `options_or_default` | `table` \| `string` | No | Configuration options table or default path string. | `{}` |
| `options.default` | `string` | No | Fallback file path if user presses Enter without typing. | `nil` |
| `options.must_exist` | `boolean` | No | Validates that the file exists on disk. | `true` |
| `options.extensions` | `string[]` \| `string` | No | Array of allowed extensions without dot (e.g. `{"json", "toml"}`). | `nil` (any extension) |

#### Examples

```lua
-- Example 1: Request path with default and extension filter
local config_path = ox.file("Path to configuration file:", {
    default = "data/config.json",
    extensions = { "json", "toml" },
    must_exist = true,
})

-- Example 2: Prompt with simple default string fallback
local dataset = ox.file("Dataset path:", "data/default.csv")

-- Example 3: Prompt for an export output path (does not need to exist yet)
local export_file = ox.file("Output CSV path:", {
    default = "reports/export.csv",
    must_exist = false,
    extensions = { "csv" },
})

-- Example 4: Named table syntax
local script_file = ox.file {
    prompt = "Path to Lua bot script:",
    extensions = "lua",
    must_exist = true,
}
```

---

## Dialog Flow

The Dialog Flow engine provides multi-step finite state machines and top-level command routing, replacing ad-hoc message listeners for structured conversations.

### `ox.flow`

Creates or accesses a named dialog flow manager.

#### Syntax

```lua
local flow = ox.flow(name[, options])
```

#### Parameters

| Parameter | Type | Required | Description | Default |
|:---|:---|:---|:---|:---|
| `name` | `string` | Yes | Unique identifier for the flow. | — |
| `options` | `table` | No | Flow configuration and conversation filters. | `{}` |
| `options.timeout` | `number` | No | Inactivity timeout in seconds before current step resets. | `nil` |
| `options.initial` | `string` | No | Explicit name of starting step. | First registered step |
| `options.chats` | `integer` \| `integer[]` | No | Restrict flow to specific chat ID(s). | `nil` |
| `options.senders` | `integer` \| `integer[]` | No | Restrict flow to specific sender ID(s). | `nil` |
| `options.incoming` | `boolean` | No | Match incoming messages (`true`) or outgoing (`false`). | `true` |
| `options.private` | `boolean` | No | Match private 1-on-1 chats. | `nil` |
| `options.group` | `boolean` | No | Match basic group chats. | `nil` |
| `options.channel` | `boolean` | No | Match supergroups and channels. | `nil` |

---

### `flow:command`

Registers a top-level slash command router that triggers an action and optionally transitions the conversation into a multi-step sequence.

#### Syntax

```lua
flow:command(commands, action)
```

#### Parameters

| Parameter | Type | Required | Description | Default |
|:---|:---|:---|:---|:---|
| `commands` | `string` \| `string[]` | Yes | Command name or list of commands without slash (e.g. `"start"` or `{"help", "info"}`). | — |
| `action` | `function(event, ctx)` | Yes | Action handler. Returning a step name transitions to that step. | — |

#### Examples

```lua
local bot = ox.flow("assistant", { private = true })

bot:command("start", function(event, ctx)
    ctx:set("attempts", 0)
    event:reply("Welcome! Please enter your access code:")
    return "verify_code"
end)

bot:command({ "help", "info" }, function(event, ctx)
    event:reply("Available commands: /start, /cancel, /status")
end)
```

---

### `flow:on`

Registers a top-level keyword or regex pattern matcher.

#### Syntax

```lua
flow:on(pattern_or_keywords, action)
```

#### Parameters

| Parameter | Type | Required | Description | Default |
|:---|:---|:---|:---|:---|
| `pattern_or_keywords` | `string` \| `string[]` | Yes | Substring or regex pattern. | — |
| `action` | `function(event, ctx)` | Yes | Handler function. Returning a step name transitions to that step. | — |

#### Examples

```lua
bot:on("cancel", function(event, ctx)
    ctx:reset()
    event:reply("Conversation cancelled.")
end)
```

---

### `flow:step`

Registers a named step in the state machine.

#### Syntax

```lua
flow:step(step_name, config)
```

#### Parameters

| Parameter | Type | Required | Description | Default |
|:---|:---|:---|:---|:---|
| `step_name` | `string` | Yes | Unique name of the step (e.g. `"ask_age"`). | — |
| `config` | `table` | Yes | Step configuration table. | — |
| `config.action` | `function(event, ctx)` | Yes | Handler executed when message arrives during this step. | — |
| `config.match` | `string` \| `string[]` | No | Substring filter required to activate the step. | `nil` |
| `config.commands` | `string` \| `string[]` | No | Slash command required to activate the step. | `nil` |
| `config.pattern` | `string` | No | Regex pattern required to activate the step. | `nil` |
| `config.timeout` | `number` | No | Per-step timeout override in seconds. | Flow timeout |
| `config.on_timeout` | `function(ctx)` | No | Callback executed when this step times out. | `nil` |

#### Context Object (`ctx`)

| Method / Field | Type | Description |
|:---|:---|:---|
| `ctx:set(key, value)` | `function` | Saves a value in this conversation's session state. |
| `ctx:get(key[, default])` | `function` | Retrieves a value from this conversation's session state. |
| `ctx.data` | `table` | Raw key-value session storage table for this conversation. |
| `ctx.current_step` | `string` | Name of the currently active step. |
| `ctx:go_to(step_name)` | `function` | Transitions immediately to another step. |
| `ctx:reset()` | `function` | Resets the conversation to the initial step. |

#### Example

```lua
local quiz = ox.flow("quiz_flow", { timeout = 60 })

quiz:command("start", function(event, ctx)
    event:reply("Question 1: What is the capital of France?")
    return "q1"
end)

quiz:step("q1", {
    action = function(event, ctx)
        if event.text:trim():lower() == "paris" then
            ctx:set("score", 1)
            event:reply("Correct! Question 2: What is 2 + 2?")
            return "q2"
        else
            event:reply("Incorrect! Try again:")
            return "q1"
        end
    end,
    on_timeout = function(ctx)
        ox.log.warn("Quiz timed out waiting for answer.")
    end,
})

quiz:step("q2", {
    action = function(event, ctx)
        local score = ctx:get("score", 0)
        if event.text:trim() == "4" then
            score = score + 1
        end
        event:reply(string.format("Quiz finished! Final score: %d/2", score))
        ctx:reset()
    end,
})
```

---

## Message Events

Every message callback receives an enriched `event` table describing the Telegram message.

### Event Properties

| Property | Type | Description |
|:---|:---|:---|
| `event.text` | `string` | Text content of the message. |
| `event.id` | `integer` | Unique integer identifier of the message. |
| `event.message_id` | `integer` | Alias for `event.id`. |
| `event.chat_id` | `integer` | Signed 64-bit peer ID of the chat / dialog. |
| `event.sender_id` | `integer` | Signed 64-bit peer ID of the sender (`0` if anonymous). |
| `event.incoming` | `boolean` | `true` if sent by another user, `false` if outgoing. |
| `event.outgoing` | `boolean` | `true` if sent from your userbot account. |
| `event.is_private` | `boolean` | `true` for 1-on-1 private user chats. |
| `event.is_group` | `boolean` | `true` for basic Telegram groups. |
| `event.is_channel` | `boolean` | `true` for broadcast channels and supergroups. |
| `event.matches` | `string[]` | Positional capture groups extracted by a regex filter. |
| `event.captures` | `table<string, string>` | Named capture groups extracted by a regex filter. |
| `event.buttons` | `Button[][]` | 2D matrix of inline or reply keyboard buttons attached to this message. |

---

### `event:reply`

Replies directly to this message with a quote (`reply_to`).

#### Syntax

```lua
event:reply(text[, options])
event:reply(options_table)
```

#### Parameters

| Parameter | Type | Required | Description | Default |
|:---|:---|:---|:---|:---|
| `text` | `string` | Yes | Reply message content. | — |
| `options` | `table` \| `number` | No | Options table or numeric delay in seconds. | `nil` |
| `options.delay` | `number` | No | Delay in seconds before sending. | `0` |
| `options.parse_mode` | `string` | No | Formatting parser: `"markdown"` or `"html"`. | `nil` |

#### Examples

```lua
-- Positional syntax
event:reply("Acknowledged!")
event:reply("*Bold acknowledgment*", { parse_mode = "markdown", delay = 0.5 })

-- Named table syntax
event:reply {
    text = "Task completed.",
    delay = 1.0,
}
```

---

### `event:edit`

Edits this message with new text.

#### Syntax

```lua
event:edit(new_text[, options])
event:edit(options_table)
```

#### Parameters

| Parameter | Type | Required | Description | Default |
|:---|:---|:---|:---|:---|
| `new_text` | `string` | Yes | New message content. | — |
| `options` | `table` \| `number` | No | Options table or delay in seconds. | `nil` |

#### Examples

```lua
event:edit("Processing finished.")
event:edit { text = "<b>Updated</b>", parse_mode = "html" }
```

---

### `event:delete`

Deletes this message.

#### Syntax

```lua
event:delete([delay])
event:delete(options_table)
```

#### Parameters

| Parameter | Type | Required | Description | Default |
|:---|:---|:---|:---|:---|
| `delay` | `number` | No | Delay in seconds before deleting. | `0` |

#### Examples

```lua
event:delete()
event:delete(3.0)
event:delete { delay = 3.0 }
```

---

### `event:react`

Sends an emoji reaction to this message.

#### Syntax

```lua
event:react(emoji[, delay])
event:react(options_table)
```

#### Parameters

| Parameter | Type | Required | Description | Default |
|:---|:---|:---|:---|:---|
| `emoji` | `string` | Yes | Reaction emoji string (e.g. `"👍"`). | — |
| `delay` | `number` | No | Delay in seconds before reacting. | `0` |

#### Examples

```lua
event:react("🔥")
event:react { emoji = "❤️", delay = 0.5 }
```

---

### `event:pin`

Pins this message in the chat.

#### Syntax

```lua
event:pin([delay])
event:pin(options_table)
```

#### Parameters

| Parameter | Type | Required | Description | Default |
|:---|:---|:---|:---|:---|
| `delay` | `number` | No | Delay in seconds before pinning. | `0` |

#### Examples

```lua
event:pin()
event:pin { delay = 1.0 }
```

---

### `event:click`

Clicks a button on this message's keyboard by text label, substring, or 1-based index.

#### Syntax

```lua
event:click(query_or_index[, delay])
event:click(options_table)
```

#### Parameters

| Parameter | Type | Required | Description | Default |
|:---|:---|:---|:---|:---|
| `query_or_index` | `string` \| `integer` | Yes | Button label text or 1-based sequential button index. | — |
| `delay` | `number` | No | Delay in seconds before clicking. | `0` |

#### Examples

```lua
-- Click by label
event:click("Accept")

-- Click the first button with delay
event:click(1, 0.5)

-- Named table syntax
event:click { query = "Next Page", delay = 1.0 }
```

---

## Storage

`ox.storage` provides persistent JSON key-value storage saved to `data/storage/<bot_name>.json`. Data is preserved across bot restarts.

### Methods

| Method | Parameters | Return Type | Description |
|:---|:---|:---|:---|
| `ox.storage.get(key, [default])` | `key: string`, `default?: any` | `any` | Retrieves a value, or `default` if missing. |
| `ox.storage.set(key, value)` | `key: string`, `value: any` | `nil` | Stores a value (string, number, boolean, or table). |
| `ox.storage.has(key)` | `key: string` | `boolean` | Checks if a key exists in storage. |
| `ox.storage.delete(key)` | `key: string` | `boolean` | Deletes a key. Returns `true` if key existed. |
| `ox.storage.all()` | — | `table` | Returns a table of all stored key-value pairs. |
| `ox.storage.clear()` | — | `nil` | Deletes all stored data for this script. |

#### Examples

```lua
-- Increment counter
local visits = ox.storage.get("visits", 0) + 1
ox.storage.set("visits", visits)
ox.log.info("Visits count: " .. visits)

-- Storing complex tables
ox.storage.set("settings", {
    auto_reply = true,
    channels = { -100111111, -100222222 },
})

local cfg = ox.storage.get("settings")
if cfg and cfg.auto_reply then
    ox.log.info("Auto-reply enabled")
end
```

---

## Utilities and Timing

### `ox.sleep`

Asynchronously pauses execution without blocking the Tokio runtime or update processing.

#### Syntax

```lua
ox.sleep(seconds)
```

#### Parameters

| Parameter | Type | Required | Description | Default |
|:---|:---|:---|:---|:---|
| `seconds` | `number` | Yes | Pause duration in seconds. | — |

#### Example

```lua
ox.send_typing(event.chat_id)
ox.sleep(1.5)
event:reply("Done.")
```

---

### `ox.sleep_random`

Pauses execution for a random duration between `min` and `max` seconds.

#### Syntax

```lua
ox.sleep_random(min, max)
```

#### Parameters

| Parameter | Type | Required | Description | Default |
|:---|:---|:---|:---|:---|
| `min` | `number` | Yes | Minimum sleep duration in seconds. | — |
| `max` | `number` | Yes | Maximum sleep duration in seconds. | — |

#### Example

```lua
-- Humanized delay between 1.0 and 3.0 seconds
ox.sleep_random(1.0, 3.0)
```

---

### `ox.choice`

Picks a random element from a Lua array table.

#### Syntax

```lua
local item = ox.choice(list)
```

#### Parameters

| Parameter | Type | Required | Description | Default |
|:---|:---|:---|:---|:---|
| `list` | `any[]` | Yes | Array table of elements. | — |

#### Example

```lua
local replies = { "Hi!", "Hello!", "Greetings!", "Welcome!" }
event:reply(ox.choice(replies))
```

---

### `ox.set_timeout` & `ox.clear_timeout`

Schedules a one-shot callback after a specified duration.

#### Syntax

```lua
local timer_id = ox.set_timeout(seconds, callback)
ox.clear_timeout(timer_id)
```

#### Parameters

| Parameter | Type | Required | Description | Default |
|:---|:---|:---|:---|:---|
| `seconds` | `number` | Yes | Duration in seconds before callback runs. | — |
| `callback` | `function()` | Yes | Function called when timer expires. | — |

#### Example

```lua
local timer_id = ox.set_timeout(10.0, function()
    ox.log.info("10-second timeout executed.")
end)

-- Cancel if needed
ox.clear_timeout(timer_id)
```

---

### `ox.set_interval` & `ox.clear_interval`

Schedules a recurring callback that executes repeatedly every interval.

#### Syntax

```lua
local timer_id = ox.set_interval(seconds, callback)
ox.clear_interval(timer_id)
```

#### Parameters

| Parameter | Type | Required | Description | Default |
|:---|:---|:---|:---|:---|
| `seconds` | `number` | Yes | Interval in seconds between executions. | — |
| `callback` | `function()` | Yes | Function called on each tick. | — |

#### Example

```lua
local ping_id = ox.set_interval(60.0, function()
    ox.log.info("Heartbeat ping: system alive.")
end)

-- Cancel with ox.clear_interval or ox.clear_timer
ox.clear_interval(ping_id)
-- ox.clear_timer(ping_id)
```

---

### `ox.stop`

Gracefully stops the bot's event loop and exits the application.

#### Syntax

```lua
ox.stop()
```

#### Example

```lua
ox.on_message({ commands = "shutdown", outgoing = true }, function(event)
    event:reply("Shutting down...")
    ox.sleep(1.0)
    ox.stop()
end)
```

---

## String Manipulation

OxideGram enhances Lua strings with metatable methods (`str:method(...)`) and a dedicated `ox.string` table (`ox.string.method(...)`).

### String Methods Reference

| Method | Parameters | Return Type | Description |
|:---|:---|:---|:---|
| `contains` | `(sub: string)` | `boolean` | Checks if string contains `sub` (case-insensitive by default). |
| `starts_with` | `(prefix: string)` | `boolean` | Checks if string begins with `prefix`. |
| `ends_with` | `(suffix: string)` | `boolean` | Checks if string concludes with `suffix`. |
| `split` | `([delimiter: string])` | `string[]` | Splits string by delimiter (defaults to whitespace). |
| `trim` | `()` | `string` | Strips leading and trailing whitespace. |
| `replace` | `(target: string, replacement: string, [is_regex: boolean])` | `string` | Replaces occurrences of `target` with `replacement`. Set `is_regex = true` for regex matching. |
| `strip_prefix` | `(prefix: string)` | `string` | Removes prefix from string if present. |
| `strip_suffix` | `(suffix: string)` | `string` | Removes suffix from string if present. |
| `pad_left` | `(length: integer, [pad_char: string])` | `string` | Pads string on the left up to `length`. |
| `pad_right` | `(length: integer, [pad_char: string])` | `string` | Pads string on the right up to `length`. |
| `is_empty` | `()` | `boolean` | Returns `true` if string is empty (`""`). |
| `is_blank` | `()` | `boolean` | Returns `true` if string is empty or contains only whitespace. |
| `escape_markdown` | `()` | `string` | Escapes special characters for Telegram MarkdownV2. |
| `escape_html` | `()` | `string` | Escapes `&`, `<`, `>`, `"` for Telegram HTML mode. |
| `extract_command` | `([bot_username: string])` | `string?, string` | Extracts command name without slash and trailing arguments. |
| `lines` | `()` | `string[]` | Splits string into an array of lines. |

#### Examples

```lua
-- 1. Cleaning and splitting input
local text = "  /ban @spammer 10m flood  "
local clean = text:trim()
local cmd, args = clean:extract_command("mybot")
-- cmd  == "ban"
-- args == "@spammer 10m flood"

-- 2. Replacing substrings (literal and regex)
local greeting = "Hello Alice!":replace("Alice", "Bob")
-- greeting == "Hello Bob!"

local masked = "Call 555-1234":replace("\\d+", "XXX", true)
-- masked == "Call XXX-XXX"

-- 3. Escaping user input for formatting
local user_bio = "Look at this: *stars* & <tags>"
local safe_md = ox.escape_markdown(user_bio)
local safe_html = ox.escape_html(user_bio)

-- 4. Padding strings
local padded = ("42"):pad_left(6, "0")
-- padded == "000042"

-- 5. Checking blank strings
local input = "   \n\t  "
if input:is_blank() then
    ox.log.warn("Empty user input provided")
end
```

---

## Logging

`ox.log` writes structured logs to stdout, the terminal console, and rotating daily log files under `data/logs/`.

### Methods

| Method | Parameters | Description |
|:---|:---|:---|
| `ox.log.info(message)` | `message: string` | Informational message. |
| `ox.log.warn(message)` | `message: string` | Warning message. |
| `ox.log.error(message)` | `message: string` | Error message. |
| `ox.log.debug(message)` | `message: string` | Detailed debugging log (visible with `--verbose`). |

#### Examples

```lua
ox.log.info("Bot loaded successfully.")
ox.log.warn("Rate limit threshold approaching.")
ox.log.error("Failed to connect to backend service.")
ox.log.debug("Internal cache state: " .. tostring(cached_count))

-- Supports colon syntax
ox.log:info("Service ready.")
```

---

## Offline Simulator

OxideGram includes a built-in interactive simulator for testing Lua bot logic locally without connecting to Telegram or requiring active credentials:

```bash
# Run simulator for a script in data/bots/
cargo run -- sim echo
```

### Simulator REPL Commands

| Command | Description |
|:---|:---|
| `<any text>` | Simulates an incoming message with the typed text. |
| `/click <text>` | Simulates clicking an inline or reply keyboard button. |
| `/buttons <b1, b2>` | Attaches mock buttons to subsequent incoming messages. |
| `/chat <id>` | Switches the simulated chat ID (e.g. `/chat 123456789`). |
| `/state` | Displays the current in-memory contents of `ox.storage`. |
| `/help` | Displays available simulator commands. |
| `/exit` | Exits the simulator REPL. |
