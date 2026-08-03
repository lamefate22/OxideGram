---
layout: default
title: OxideGram Scripting Engine
---

# OxideGram Scripting Engine

OxideGram embeds Lua 5.4 through `mlua`. A script registers message handlers, describes which Telegram messages each handler accepts, and sends responses through the global `ox` API.

[Quick start](#quick-start) | [API](api.html) | [Filters](filters.html) | [Examples](examples.html)

## Quick Start

Create `data/bots/hello.lua`:

```lua
ox.on_message({ commands = "hello", incoming = true }, function(event)
    ox.send_message(event.chat_id, "Hello from Lua!", 0)
end)
```

Run OxideGram from the project root:

```bash
cargo run
```

After authorization, select `hello` from the script list. Scripts are loaded once at startup; restart OxideGram after changing the selected script.

## Script Lifecycle

1. OxideGram scans `data/bots/` for files whose extension is exactly `.lua`.
2. The selected file is evaluated in a fresh embedded Lua state.
3. Calls to `ox.on_message` register callbacks and their optional filters.
4. OxideGram starts the Telegram update stream.
5. Every new message is converted into an event table and dispatched to each matching handler.
6. A handler error is logged and does not prevent later handlers from running.

There is currently no hot reload, timer API, media API, edit/delete handler, or module-level persistent storage provided by OxideGram.

## Minimal Echo Script

```lua
ox.on_message({ incoming = true, has_text = true }, function(event)
    local response = "You said: " .. event.text
    ox.send_message(event.chat_id, response, 0)
end)
```

## Next Steps

- Read the [API reference](api.html) for functions and event fields.
- Read the [filter reference](filters.html) for accepted values and matching rules.
- Browse [complete examples](examples.html). Copy-ready versions are also stored in the repository's `examples/bots/` directory.

## Compatibility

`nox` is currently exposed as an alias of `ox` for scripts inherited from NoxGram. New scripts should use `ox`.

The scripting surface is under development. Pin scripts to a known OxideGram version when using the project in long-running automation.
