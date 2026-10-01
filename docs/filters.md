---
layout: default
title: Message Filters
---

# Message Filters

[Home](index.html) | [API](api.html) | [Examples](examples.html)

Filters are defined as Lua tables passed as the first argument to `ox.on_message`. They allow you to declaratively filter Telegram message updates so your callbacks only execute when all criteria match.

```lua
ox.on_message({
    incoming = true,
    private = true,
    has_text = true,
}, function(event)
    -- Executes only for incoming text messages in 1-on-1 private chats
end)
```

Filters use logical **AND** semantics across different keys: every specified key must match. To express logical **OR** for values of the same key (such as multiple chat IDs or command names), provide a sequential array.

---

## Filter Reference

| Key | Accepted Types | Description |
| :--- | :--- | :--- |
| `chats` | `integer` or `integer[]` | Matches when `event.chat_id` equals one of the specified peer IDs. |
| `senders` | `integer` or `integer[]` | Matches when `event.sender_id` equals one of the specified peer IDs. |
| `incoming` | `boolean` | Matches whether the message was sent by another account. |
| `outgoing` | `boolean` | Matches whether the message was sent by your userbot account. |
| `private` | `boolean` | Matches 1-on-1 private user dialogs. |
| `group` | `boolean` | Matches basic group chats. |
| `channel` | `boolean` | Matches broadcast channels and supergroups. |
| `has_text` | `boolean` | Matches messages with non-empty text content (or empty if `false`). |
| `commands` | `string` or `string[]` | Matches messages starting with one of the specified slash commands. |
| `pattern` | `string` | Matches messages against a Rust-compatible regular expression. |

---

## Chat and Sender Filters

Match a single chat or user peer:

```lua
-- Only match messages in chat ID 123456789
{ chats = 123456789 }
```

Match any chat or sender from an allowed whitelist:

```lua
{
    chats = { 123456789, 987654321 },
    senders = { 111111111, 222222222 },
}
```

> [!NOTE]
> Telegram peer IDs are signed 64-bit integers. Supergroups and channels in MTProto usually have negative 64-bit identifiers (e.g. `-1001234567890`). You can inspect `event.chat_id` and `event.sender_id` inside a general handler to discover IDs.

---

## Message Direction

```lua
-- Only incoming messages (sent by others)
{ incoming = true }

-- Only outgoing messages (sent by your account)
{ outgoing = true }

-- Inverted check: any message that is not outgoing
{ outgoing = false }
```

> [!TIP]
> `incoming` and `outgoing` are mutually exclusive for a given message. Do not set both to `true` on the same handler, as that condition will never match.

---

## Chat Types

```lua
{ private = true } -- 1-on-1 user chats
{ group = true }   -- Basic group chats
{ channel = true } -- Supergroups and broadcast channels
```

To handle messages across both private chats and basic groups with the same logic, register two handlers sharing the same callback function:

```lua
local function handle_dialog(event)
    ox.log.info(string.format("Chat %d: %s", event.chat_id, event.text))
end

ox.on_message({ private = true, incoming = true }, handle_dialog)
ox.on_message({ group = true, incoming = true }, handle_dialog)
```

---

## Text Presence

```lua
-- Only process messages that contain textual content
{ has_text = true }

-- Only process media messages that have no text or caption
{ has_text = false }
```

`has_text` checks whether `event.text` is non-empty. It matches both standalone text messages and media captions.

---

## Command Matching

```lua
-- Single command match
{ commands = "help" }

-- Multiple command aliases
{ commands = { "start", "menu", "/help" } }
```

Features of command filtering:
- Leading slashes (`/`) are optional in configuration (`"start"` matches `/start`).
- Matching is case-insensitive (`/start`, `/START`, and `/Start` all match).
- Handles bot username suffixes automatically (`/start@my_bot` matches `"start"`).
- Arguments after the command are preserved in `event.text` for custom parsing.

---

## Regular Expressions and Named Captures

`pattern` compiles a regular expression using Rust's high-performance [`regex`](https://docs.rs/regex/latest/regex/) engine:

```lua
-- Case-insensitive whole word search
{ pattern = "(?i)\\balert\\b" }

-- Command with structured arguments
{ pattern = "^/ban\\s+(?<target>@\\w+)\\s+(?<reason>.+)$" }
```

### Accessing Capture Groups

When a pattern matches, captured groups are automatically populated on the `event` object:
- `event.matches[1]`, `event.matches[2]`: Positional numerical capture groups (1-based index).
- `event.captures["name"]`: Named capture groups defined via `(?<name>...)`.

```lua
ox.on_message({
    pattern = "^/set\\s+(?<key>[a-zA-Z0-9_]+)\\s+(?<value>.+)$",
}, function(event)
    local key = event.captures.key
    local value = event.captures.value

    ox.storage.set(key, value)
    event:reply(string.format("Stored: `%s` = `%s`", key, value), {
        parse_mode = "markdown",
    })
end)
```

> [!IMPORTANT]
> Because backslashes in Lua string literals represent escape characters, escape regex backslashes using `\\` (e.g. `"\\d+"` or `"\\s+"`). Patterns are compiled once upon script loading. Syntax errors in regex patterns cause immediate startup validation errors with descriptive line diagnostics.

---

## Combined Filter Examples

### Whitelisted Private Command Handler

```lua
ox.on_message({
    incoming = true,
    private = true,
    senders = { 111111111, 222222222 },
    commands = { "admin", "status" },
}, function(event)
    event:reply("Authorized admin command recognized.")
end)
```

### Channel Keyword Monitor

```lua
ox.on_message({
    incoming = true,
    channel = true,
    pattern = "(?i)\\b(urgent|critical|security)\\b",
}, function(event)
    ox.log.warn(string.format("Urgent keyword detected in channel %d: %s", event.chat_id, event.text))
    event:react("🚨")
end)
```

---

## Validation Rules

1. `chats` and `senders` must be signed 64-bit integers or non-empty sequential integer arrays.
2. `commands` must be a string or a non-empty sequential string array.
3. Boolean filters accept strictly `true` or `false`.
4. `pattern` must be a valid regular expression string.
5. All filter arrays must be sequential 1-based Lua tables (`ipairs` compatible).
