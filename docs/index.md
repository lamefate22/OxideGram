---
layout: default
title: OxideGram Scripting Engine
---

# OxideGram Scripting Engine

OxideGram is a high-performance, resilient Telegram userbot and automation engine built in Rust, powered by embedded Lua 5.4. It allows you to write declarative event handlers, simulate human interactions (including inline and reply keyboard button clicks), build dialog state machines (`ox.flow`), run multi-session clusters, and test scripts offline in a hot-reloading simulator.

[Quick start](#quick-start) | [CLI Reference](#cli-reference) | [Hardware-Bound Vault](#hardware-bound-vault) | [Dual-Layer Tracing](#dual-layer-tracing) | [API](api.html) | [Filters](filters.html) | [Examples](examples.html)

---

## Quick Start

Create `data/bots/hello.lua` (or use the built-in template generator `cargo run -- template create echo`):

```lua
ox.on_message({ commands = "hello", incoming = true }, function(event)
    event.reply("Hello from OxideGram userbot! 🚀", { parse_mode = "markdown" })
end)
```

Run OxideGram directly via the CLI:

```bash
# Launch a specific bot immediately
cargo run -- run hello

# Or test it offline in the dry-run simulator
cargo run -- sim hello

# Or launch the interactive terminal menu
cargo run
```

---

## CLI Reference

OxideGram features a modern, ergonomic CLI powered by `clap` v4:

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

### Common Command Examples

| Task | Command |
| :--- | :--- |
| **Launch bot directly** | `oxidegram run hello` |
| **Launch with specific phone** | `oxidegram run hello -s +1234567890` |
| **Offline simulator** | `oxidegram sim hello` |
| **Lint & validate scripts** | `oxidegram check` |
| **Multi-bot cluster** | `oxidegram cluster bot1 bot2 bot3` |
| **List saved sessions** | `oxidegram session list` |
| **Add new session** | `oxidegram session add` |
| **Delete session** | `oxidegram session remove +1234567890` |
| **Check hardware vault** | `oxidegram session unlock` |
| **Lock / clear vault** | `oxidegram session lock` |
| **Generate template** | `oxidegram template create flow --name my_dialog` |

---

## Hardware-Bound Vault (Zero Friction)

Tired of typing your master encryption password on every single launch? OxideGram features a **Hardware-Bound Device Vault** (`data/.device_vault`):

- **No external OS keyring daemons required**: Works seamlessly on headless servers and mobile environments where GNOME Keyring or DBus are unavailable.
- **Cross-Platform Host Binding**:
  - **Windows**: MachineGuid (`HKLM\SOFTWARE\Microsoft\Cryptography`), Motherboard Serial, CPU Identifier, System Volume Serial.
  - **Linux**: `/etc/machine-id`, CPU model/vendor, Hostname, SMBIOS DMI tables.
  - **Android (Termux)**: `ro.build.fingerprint`, SoC Hardware, Termux sandbox UID/inode.
- **Cryptographic Security**: Salted Argon2id key derivation combined with authenticated AES-256-GCM encryption. On Unix/Android, files are strictly protected with `chmod 0600`.
- **Automatic Unlock**: When you run OxideGram, it automatically unlocks your saved sessions if the hardware fingerprint matches. If transferred to another device, the vault safely refuses to open and prompts for your master password.
- **One-Session Auto-Skip**: If you only have one authorized Telegram account saved, OxideGram automatically selects it without prompting.

---

## Dual-Layer Tracing

OxideGram uses a dual-layer logging system powered by `tracing` and `tracing-appender`:

1. **Compact, Clean Console**:
   - MTProto ping and network heartbeats are filtered out.
   - Significant actions are formatted in single, easy-to-read lines:
     - `[MSG]  [+1234567890] @target_user (ID: 987654) -> "Hello!"`
     - `[ACT]  [+1234567890] Clicked inline button "Confirm" on msg 1042`
     - `[FLOW] [+1234567890] Transition: flow 'quiz' [question_1] -> [question_2]`
     - `[LUA]  [+1234567890] [INFO] Custom user script message`
2. **Detailed File Audit Log**:
   - Full debug traces, MTProto state events, and stack traces are non-blockingly written to `data/logs/oxidegram.log` with daily rotation.
   - Customizable via environment variables `OXIDEGRAM_LOG` (console) and `OXIDEGRAM_FILE_LOG` (file).

---

## Key Features

- **Live Hot-Reload**: Save changes in your `.lua` file, and OxideGram reloads the script on the fly without breaking your active MTProto connection or logging in again.
- **Offline Dry-Run Simulator**: Test bot logic, button clicking, and step flows entirely offline with hot-reload and an interactive REPL before running on real accounts.
- **Dialog Flow FSM Engine**: Create declarative multi-step dialogs (`ox.flow`) with automatic step timeouts, triggers, and state transitions.
- **Persistent Storage**: Save key-value data between bot restarts effortlessly with `ox.storage`.
- **String Helpers & Stdlib**: Built-in string methods (`str:contains`, `str:starts_with`, `str:split`, `str:trim`) and humanized delays (`ox.sleep_random`, `ox.choice`).
- **Inline & Reply Keyboard Button Clicking**: Full support for Telegram Inline and Reply keyboards (`event.buttons`), with programmatic click simulation (`event.click("Verify")` or `event.click(1)`).
- **Rich Message Control**: Edit messages (`event.edit`), delete (`event.delete`), send emoji reactions (`event.react`), pin (`event.pin`), and forward (`ox.forward_message`).
- **Media & Formatting**: Send photos, documents (`ox.send_document`), audio tracks (`ox.send_audio`), and voice messages (`ox.send_voice`) with Markdown and HTML entity support.
- **Regex Captures & Timers**: Extract regex positional (`event.matches`) and named capture groups (`event.captures`), and run background intervals or timeouts (`ox.set_interval`, `ox.set_timeout`).
- **Anti-Spam & Humanization**: Automatic `FloodWait` backoff retry, human-like delay jitter, and typing simulation (`ox.send_typing`).
- **Multi-Session Clustering**: Run isolated bot tasks across multiple Telegram accounts with graceful Ctrl+C shutdown.

---

## Script Lifecycle

1. OxideGram scans `data/bots/` for `.lua` script files.
2. The selected script is evaluated in an isolated embedded Lua 5.4 runtime.
3. Startup prompts (`ox.input`) configure variables before the event loop starts.
4. Telegram updates are received non-blockingly over a background connection.
5. Matching messages are dispatched to registered handlers along with enriched event helpers.
6. When editing `.lua` files on disk, the filesystem watcher triggers an automatic reload without disconnecting from Telegram.
7. The process runs until `ox.stop()` is called or the user interrupts it with `Ctrl+C`.

---

## Next Steps

- Read the [API Reference](api.html) for all available `ox` and `event` methods.
- Read the [Filter Reference](filters.html) for accepted keys and matching behavior.
- Explore [Complete Examples](examples.html) for button clicking, timers, and regex workflows.
