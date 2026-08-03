---
layout: default
title: Script Examples
---

# Script Examples

[Documentation home](index.html) | [API](api.html) | [Filters](filters.html)

Copy-ready versions of these scripts live in the repository's `examples/bots/` directory. Copy a file into `data/bots/`, start OxideGram, and select it from the terminal menu.

## Echo Incoming Text

```lua
ox.on_message({ incoming = true, has_text = true }, function(event)
    ox.send_message(event.chat_id, "You said: " .. event.text, 0)
end)
```

This is intentionally broad. Add `private = true`, `chats`, or `senders` before using it on a busy account.

## Command Router

```lua
ox.on_message({ incoming = true, commands = "start" }, function(event)
    ox.send_message(event.chat_id, "Welcome. Try /help.", 0)
end)

ox.on_message({ incoming = true, commands = "help" }, function(event)
    ox.send_message(event.chat_id, "Available commands: /start, /help", 0)
end)
```

## Private Auto-Responder

```lua
ox.on_message({
    incoming = true,
    private = true,
    has_text = true
}, function(event)
    ox.send_message(event.chat_id, "I am currently away. I will reply later.", 1)
end)
```

Restrict this with `senders` or `chats` while testing to avoid replying to every private dialog.

## Startup Configuration And Photo

```lua
local image_path = ox.input("Image path")
local caption = ox.input("Caption", "Sent by OxideGram")

ox.on_message({ incoming = true, private = true, commands = "photo" }, function(event)
    ox.send_image(event.chat_id, image_path, caption, 1)
end)
```

Input prompts run while the script loads. Keep them outside message handlers.

## Pattern Matcher

```lua
ox.on_message({
    incoming = true,
    has_text = true,
    pattern = "(?i)\\boxidegram\\b"
}, function(event)
    ox.send_message(event.chat_id, "OxideGram was mentioned.", 0)
end)
```

## Shared Callback For Multiple Chat Types

```lua
local function greet(event)
    ox.send_message(event.chat_id, "Hello!", 0)
end

ox.on_message({ incoming = true, private = true, commands = "hello" }, greet)
ox.on_message({ incoming = true, group = true, commands = "hello" }, greet)
```

Registering twice expresses OR between chat types while preserving AND inside each filter table.

## Stop On A Message

```lua
local ADMIN_ID = 123456789

ox.on_message({ senders = ADMIN_ID, incoming = true, commands = "stop" }, function(event)
    ox.send_message(event.chat_id, "Bot stopped", 0)
    ox.stop()
    return
end)
```

Restrict remote stop handlers by `senders` or `chats`. The current handler finishes before the event loop exits.

## Inspect Event Metadata

```lua
ox.on_message(function(event)
    print("text:", event.text)
    print("chat_id:", event.chat_id)
    print("sender_id:", event.sender_id)
    print("incoming:", event.incoming)
    print("outgoing:", event.outgoing)
    print("private:", event.is_private)
    print("group:", event.is_group)
    print("channel:", event.is_channel)
end)
```

This is useful for discovering IDs, but standard Lua output shares the interactive terminal. Remove or restrict the handler after debugging.

## Safer Testing

Start with your own chat or a dedicated test account:

```lua
local TEST_CHAT = 123456789

ox.on_message({ chats = TEST_CHAT, incoming = true }, function(event)
    ox.send_message(event.chat_id, "Test handler received: " .. event.text, 0)
end)
```

Replace the placeholder with an actual `event.chat_id`. Avoid unrestricted auto-replies until behavior has been verified.
