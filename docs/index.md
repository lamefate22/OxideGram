---
layout: default
title: OxideGram Documentation
---

# OxideGram Documentation

[Overview](#overview) | [Quick Start](#quick-start) | [CLI Reference](#command-line-interface) | [Session Vault](#session-vault-and-encryption) | [Logging System](#dual-layer-logging-system) | [API Reference](api.html) | [Filters](filters.html) | [Examples](examples.html)

---

## Overview

OxideGram is an asynchronous, high-performance Telegram userbot engine written in Rust with an embedded Lua 5.4 scripting environment. It provides a declarative event model, human interaction simulation (including inline and reply keyboard clicks), a finite state machine dialog engine (`ox.flow`), multi-account session management with hardware-bound encryption, and an offline dry-run simulator.

---

## Quick Start

### 1. Create a Bot Script

Create a script file in `data/bots/echo.lua` (or generate one using `oxidegram template create echo`):

```lua
ox.on_message({ commands = "echo", incoming = true }, function(event)
    event:reply("Hello from OxideGram userbot! 🚀", {
        parse_mode = "markdown",
        delay = 0.5,
    })
end)
```

### 2. Launch the Engine

```bash
# Execute a specific bot script
oxidegram run echo

# Or test the script offline in the interactive simulator
oxidegram sim echo

# Or launch the interactive terminal selection menu
oxidegram
```

---

## Command-Line Interface

OxideGram provides a command-line interface powered by `clap` v4:

```text
Usage: oxidegram [OPTIONS] [COMMAND]

Commands:
  run       Run a bot script with an active Telegram session
  sim       Test a bot script offline in the interactive dry-run simulator
  check     Analyze and validate bot scripts for syntax and deprecations
  cluster   Run multiple bots concurrently across saved Telegram sessions
  session   Manage stored Telegram sessions and hardware vault
  template  Generate or list ready-to-use bot script templates
  help      Print this message or the help of the given subcommand(s)

Options:
  -v, --verbose                  Enable verbose debug output in the terminal
  -l, --log-level <LOG_LEVEL>    Terminal log verbosity [default: compact] [possible values: compact, detailed, silent]
  -p, --password <PASSWORD>      Master password to decrypt sessions (or use OXIDEGRAM_PASSWORD env)
  -h, --help                     Print help
  -V, --version                  Print version
```

### Common Commands

| Purpose | Command |
| :--- | :--- |
| **Run bot** | `oxidegram run hello` |
| **Run bot with specific phone** | `oxidegram run hello -s +1234567890` |
| **Run offline simulator** | `oxidegram sim hello` |
| **Validate all scripts** | `oxidegram check` |
| **Launch multi-bot cluster** | `oxidegram cluster bot1 bot2 bot3` |
| **List saved accounts** | `oxidegram session list` |
| **Authorize new account** | `oxidegram session add` |
| **Remove account** | `oxidegram session remove +1234567890` |
| **Unlock hardware vault** | `oxidegram session unlock` |
| **Lock hardware vault** | `oxidegram session lock` |
| **Generate script template** | `oxidegram template create flow --name my_dialog` |

---

## Session Vault and Encryption

OxideGram protects saved Telegram authorization sessions using authenticated **AES-256-GCM** encryption with **Argon2id** key derivation.

To eliminate repetitive password prompts on trusted machines, OxideGram includes a **Hardware-Bound Device Vault** (`data/.device_vault`):

- **Zero External Dependencies**: Operates seamlessly on headless servers and minimal containers without requiring external keyring daemons (such as DBus or GNOME Keyring).
- **Hardware Probing**:
  - **Windows**: MachineGuid (`HKLM\SOFTWARE\Microsoft\Cryptography`), Motherboard Serial, CPU Identifier, and System Volume Serial.
  - **Linux**: `/etc/machine-id`, CPU model/vendor, Hostname, and SMBIOS DMI tables.
  - **Android (Termux)**: Build fingerprint, hardware SoC name, and application sandbox UID.
- **Automatic Session Selection**: If only one authorized Telegram account is stored, OxideGram automatically selects it on startup without extra menu prompts.
- **Tamper Resistance**: If the session storage or vault file is copied to an unauthenticated machine, the hardware fingerprint mismatch prevents decryption and prompts for the master password.

---

## Dual-Layer Logging System

OxideGram provides a dual-layer logging architecture powered by `tracing` and `tracing-appender`:

1. **Terminal Output (Console)**:
   - High-noise MTProto ping packets and internal transport details are suppressed.
   - User actions and critical events appear as concise single-line entries:
     - `[MSG]  [+1234567890] @target_user (ID: 987654) -> "Hello!"`
     - `[ACT]  [+1234567890] Clicked inline button "Confirm" on msg 1042`
     - `[FLOW] [+1234567890] Step transition: 'quiz' [question_1] -> [question_2]`
     - `[LUA]  [+1234567890] [INFO] Custom user script message`
2. **File Audit Log**:
   - Comprehensive debug traces, state transitions, and network diagnostics are written asynchronously to `data/logs/` with daily rotation.
   - Verbosity levels can be adjusted using the `OXIDEGRAM_LOG` and `OXIDEGRAM_FILE_LOG` environment variables or `--verbose` flag.

---

## Key Features

- **Live Hot-Reload**: Editing and saving `.lua` scripts instantly updates bot logic in memory without disconnecting active MTProto connections.
- **Interactive Offline Simulator**: Test button clicking, regex routing, and dialog state transitions in a local terminal REPL without network connectivity.
- **Dialog State Machine (`ox.flow`)**: Construct complex multi-step dialogs with per-step timeouts, branching conditions, and persistent state.
- **Persistent Key-Value Storage (`ox.storage`)**: Store user preferences, counters, and session states in automatic JSON storage files (`data/storage/`).
- **Interactive Startup Prompts**: Request runtime inputs via interactive terminal select menus (`ox.select`), yes/no confirmations (`ox.confirm`), file path prompts (`ox.file`), and text prompts (`ox.input`).
- **Keyboard Button Automation**: Comprehensive support for inline and reply keyboards (`event.buttons`), with programmatic click execution (`event:click`).
- **Rich Message Control**: Edit messages (`event:edit`), delete (`event:delete`), apply emoji reactions (`event:react`), pin messages (`event:pin`), and forward (`ox.forward_message`).
- **Media File Transfers**: Send photos, uncompressed files, audio tracks, and native voice notes (`ox.send_image`, `ox.send_document`, `ox.send_audio`, `ox.send_voice`).
- **Regex Captures and Background Timers**: Extract positional (`event.matches`) and named captures (`event.captures`), and manage background intervals and timeouts (`ox.set_interval`, `ox.set_timeout`).

---

## IDE Integration and Types

OxideGram includes full **LuaCATS** (Lua Language Server / EmmyLua) type definitions located in `types/oxidegram.d.lua`.

When opening the project in **Zed** (via `.zed/settings.json`), **Visual Studio Code** (with the `sumneko.lua` extension), or **Neovim** (via `.luarc.json`):
- **Autocomplete & Signatures**: Type `ox.` or `event:` to see all available methods, descriptions, parameter types, and inline code examples.
- **Type Checking**: Full diagnostics for configuration tables, message options, and event properties.
- **Zero Configuration Warnings**: Global symbols (`ox`) and string extensions are pre-configured in project settings.

---

## Execution Lifecycle

1. OxideGram discovers script files located inside `data/bots/`.
2. The selected script is evaluated in an isolated Lua 5.4 environment.
3. Startup configuration prompts (`ox.select`, `ox.confirm`, `ox.input`) gather operational parameters if defined.
4. Updates from Telegram are processed asynchronously over MTProto background tasks.
5. Incoming messages matching filter criteria execute registered handler functions.
6. When `.lua` files change on disk, the filesystem watcher automatically reloads handlers without terminating the connection.
7. Execution terminates gracefully on `ox.stop()` or when receiving an interrupt signal (`Ctrl+C`).

---

## Next Steps

- Explore the [API Reference](api.html) for detailed signatures and method examples.
- Review [Message Filters](filters.html) for filter conditions, chat whitelisting, and regex patterns.
- Study [Script Examples](examples.html) for end-to-end automation recipes.
