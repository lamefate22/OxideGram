---
layout: default
title: Message Filters
---

# Message Filters

[Documentation home](index.html) | [API](api.html) | [Examples](examples.html)

Filters are passed as the first argument to `ox.on_message`:

```lua
ox.on_message({ incoming = true, private = true }, function(event)
    -- Runs only for incoming private messages.
end)
```

Every specified condition must match. In other words, filters use logical AND. To express OR for values of the same kind, use an array such as `chats = { 100, 200 }`. To express more complex alternatives, register multiple handlers.

## Reference

| Filter | Accepted value | Matches when |
| --- | --- | --- |
| `chats` | integer or non-empty integer array | `event.chat_id` equals one of the IDs. |
| `senders` | integer or non-empty integer array | `event.sender_id` equals one of the IDs. |
| `incoming` | boolean | The message's incoming state equals the value. |
| `outgoing` | boolean | The message's outgoing state equals the value. |
| `private` | boolean | The dialog is, or is not, a private user dialog. |
| `group` | boolean | The dialog is, or is not, a basic group. |
| `channel` | boolean | The dialog is, or is not, a channel or supergroup. |
| `has_text` | boolean | Message text is non-empty, or empty when set to `false`. |
| `commands` | string or non-empty string array | Text starts with one of the slash commands. |
| `pattern` | string | The Rust regular expression matches `event.text`. |

## Chat And Sender IDs

Match one dialog:

```lua
{ chats = 123456789 }
```

Match any listed dialog and sender:

```lua
{
    chats = { 123456789, 987654321 },
    senders = { 111111111, 222222222 }
}
```

IDs are bare signed 64-bit integers as exposed in `event.chat_id` and `event.sender_id`. Log or temporarily print these event fields to discover values for your account. Do not assume IDs copied from Bot API examples have identical formatting.

## Direction

```lua
{ incoming = true }
{ outgoing = true }
```

`incoming` and `outgoing` are opposites for a message. Usually specify only one. Contradictory combinations such as `{ incoming = true, outgoing = true }` never match.

Boolean filters can also exclude a condition:

```lua
-- Any message that is not outgoing.
{ outgoing = false }
```

## Chat Type

```lua
{ private = true }
{ group = true }
{ channel = true }
```

In the current Telegram peer model:

- `private` corresponds to a user peer.
- `group` corresponds to a basic group chat peer.
- `channel` corresponds to both broadcast channels and supergroups.

To match private messages or basic groups, register two handlers or use a shared callback:

```lua
local function handle_dialog(event)
    print(event.chat_id, event.text)
end

ox.on_message({ private = true }, handle_dialog)
ox.on_message({ group = true }, handle_dialog)
```

## Text Presence

```lua
{ has_text = true }
```

This checks only whether `event.text` is non-empty. It does not distinguish plain text from a caption or inspect media.

## Commands

```lua
{ commands = "start" }
{ commands = { "start", "help", "/about" } }
```

Command names may be written with or without the leading `/`. Matching is case-insensitive and supports arguments and bot suffixes:

- `/start`
- `/START`
- `/start argument`
- `/start@my_bot argument`

All match `commands = "start"`. Text must begin with `/`; plain `start` does not match.

## Regular Expressions

`pattern` uses the Rust [`regex`](https://docs.rs/regex/latest/regex/) syntax and searches anywhere in the message by default:

```lua
{ pattern = "hello" }
{ pattern = "(?i)^hello[!.]?$" }
{ pattern = "^/item\\s+[0-9]+$" }
```

Remember that Lua string escaping is applied before the regular expression is parsed, so a regex backslash usually appears as `\\` in a quoted Lua string.

Patterns are compiled once while loading the script. An invalid expression aborts loading instead of silently falling back to substring matching.

## Combined Example

```lua
ox.on_message({
    chats = { 123456789, 987654321 },
    senders = 111111111,
    incoming = true,
    private = true,
    has_text = true,
    commands = { "start", "help" },
    pattern = "^/"
}, function(event)
    ox.send_message(event.chat_id, "Accepted: " .. event.text, 0)
end)
```

This handler runs only when every listed condition matches.

## Validation Rules

- `chats` and `senders` must be an integer or a non-empty sequential integer array.
- `commands` must be a string or a non-empty sequential string array.
- Boolean filters accept only `true` or `false`.
- `pattern` must be a valid string containing a valid Rust regular expression.
- Lua array indexes should be sequential, starting at `1`.
- Unknown filter keys are currently ignored. Treat misspelled names as an error in your own review because they result in a broader handler than intended.
