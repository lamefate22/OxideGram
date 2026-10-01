---
layout: default
title: OxideGram Scripting Engine
---

# OxideGram Scripting Engine

OxideGram embeds Lua 5.4 with LuaJIT/asynchronous runtime support via `mlua`. A script registers message handlers, describes which Telegram messages each handler accepts, simulates user interactions (including inline button clicking), and executes automated workflows through the global `ox` API.

[Quick start](#quick-start) | [API](api.html) | [Filters](filters.html) | [Examples](examples.html)

## Quick Start

Create `data/bots/hello.lua` (or use the built-in template generator in OxideGram menu):

```lua
ox.on_message({ commands = "hello", incoming = true }, function(event)
    event.reply("Hello from OxideGram userbot! 🚀", { parse_mode = "markdown" })
end)
```

Run OxideGram from the project root:

```bash
cargo run
```

In the interactive menu, you can:
1. **Run Single Bot**: authorize or choose a session and launch your script.
2. **Run Multi-Session Cluster**: run multiple bots across multiple Telegram accounts concurrently.
3. **Create Bot Script Template**: instantly scaffold ready-to-run bots (`echo`, `buttons`, `full`).

## Key Features

- **Live Hot-Reload**: Save changes in your `.lua` file, and OxideGram reloads the script on the fly without breaking your active MTProto connection or logging in again.
- **Button Markup & Click Simulation**: Full support for Telegram Inline and Reply keyboards (`event.buttons`), with programmatic click simulation via MTProto `GetBotCallbackAnswer` (`event.click("Verify")`).
- **Rich Message Control**: Edit messages (`event.edit`), delete (`event.delete`), send emoji reactions (`event.react`), pin (`event.pin`), and forward (`ox.forward_message`).
- **Media & Formatting**: Send photos, documents (`ox.send_document`), audio tracks (`ox.send_audio`), and voice messages (`ox.send_voice`) with Markdown and HTML entity support.
- **Regex Captures & Timers**: Extract regex positional (`event.matches`) and named capture groups (`event.captures`), and run background intervals or timeouts (`ox.set_interval`, `ox.set_timeout`).
- **Anti-Spam & Humanization**: Automatic `FloodWait` backoff retry, human-like delay jitter, and typing simulation (`ox.send_typing`).
- **Multi-Session Clustering**: Run isolated bot tasks across multiple Telegram accounts with graceful Ctrl+C shutdown.

## Script Lifecycle

1. OxideGram scans `data/bots/` for `.lua` script files.
2. The selected script is evaluated in an isolated embedded Lua 5.4 runtime.
3. Startup prompts (`ox.input`) configure variables before the event loop starts.
4. Telegram updates are received non-blockingly over a background connection.
5. Matching messages are dispatched to registered handlers along with enriched event helpers.
6. When editing `.lua` files on disk, the filesystem watcher triggers an automatic reload without disconnecting from Telegram.
7. The process runs until `ox.stop()` is called or the user interrupts it with `Ctrl+C`.

## Minimal Echo Script

```lua
ox.on_message({ incoming = true, has_text = true }, function(event)
    local response = "You said: " .. event.text
    event.reply(response, { delay = 0.5 })
end)
```

## Next Steps

- Read the [API Reference](api.html) for all available `ox` and `event` methods.
- Read the [Filter Reference](filters.html) for accepted keys and matching behavior.
- Explore [Complete Examples](examples.html) for button clicking, timers, and regex workflows.
