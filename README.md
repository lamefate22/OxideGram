# OxideGram

OxideGram is a terminal-first Telegram automation client written in Rust. It connects directly to Telegram through MTProto and runs user-defined Lua scripts for message handling.

The project is a CLI-focused rewrite of [NoxGram](https://github.com/lamefate22/NoxGram). It is currently in an early development stage, so the scripting API may still evolve.

## Features

- Telethon-like message filters for chats, senders, direction, chat type, commands, text, and regular expressions
- Argon2id password-based key derivation and AES-256-GCM authenticated encryption
- Minimal dark-terminal interface with consistent interactive prompt styling
- Native Telegram user authorization through MTProto, powered by `grammers`
- Embedded Lua 5.4 runtime, with no separate Lua installation required
- Interactive terminal login with SMS codes and Telegram 2FA support
- Cross-platform Rust codebase for Windows, Linux, and macOS
- Multiple saved accounts with encrypted local sessions
- Automatic discovery of `.lua` scripts in `data/bots/`
- Structured daily log files with seven-file retention

## Requirements

To build OxideGram from source, install:

- A Telegram `API_ID` and `API_HASH` from [my.telegram.org](https://my.telegram.org/)
- Rust with Cargo and a toolchain that supports Rust 2024 edition
- A C/C++ build toolchain required by native dependencies
- Git

Common native toolchains are Visual Studio Build Tools on Windows, `build-essential` on Debian/Ubuntu, and Xcode Command Line Tools on macOS.

## Download And Install

Prebuilt packages can be downloaded from the repository's **Releases** page when releases are available. Extract the archive and run the `OxideGram` executable from its directory so the adjacent `data/` directory can be used.

To build the latest source version:

```bash
git clone https://github.com/lamefate22/OxideGram
cd OxideGram
cargo build --release
```

The executable is created at:

- Windows: `target/release/OxideGram.exe`
- Linux and macOS: `target/release/OxideGram`

You can also start the development build directly:

```bash
cargo run
```

## First Run

1. Place at least one Lua script in `data/bots/`. The repository includes `data/bots/echo.lua`.
2. Start OxideGram from the repository or distribution root.
3. Choose manual login on the first run.
4. Enter your phone number, Telegram `API_ID`, `API_HASH`, login code, and 2FA password if requested.
5. Choose a password used to encrypt the saved session locally.
6. Select a discovered Lua script from the terminal menu.

On later runs, auto-login lets you select a saved account and unlock it with its encryption password.

OxideGram creates runtime files relative to its current working directory:

```text
data/
|-- bots/          # Lua scripts discovered at startup
|-- logs/          # Daily application logs, kept for seven days
|-- sessions/      # Working SQLite session files
`-- config.oxide   # Account metadata and encrypted session payloads
```

Do not publish `data/config.oxide` or `data/sessions/`. They contain account and session information. These paths are excluded by the repository's `.gitignore`.

## Writing A Bot

Create a `.lua` file under `data/bots/` and register one or more handlers:

```lua
ox.on_message({
    incoming = true,
    commands = { "hello", "hi" }
}, function(event)
    event:reply("Hello from OxideGram!")
end)
```

All specified filters are combined with logical AND.

See the [scripting engine documentation](docs/index.md) for the complete API, filter reference, event fields, behavior notes, and examples. Copy-ready scripts are available in [`examples/bots/`](examples/bots/).

## Logging

Logs are written to `data/logs/oxidegram*.log` and rotate daily. Up to seven files are retained. Terminal output remains focused on interactive prompts and fatal errors.

The default filter records OxideGram messages at `info` level and warnings from dependencies. Set `OXIDEGRAM_LOG` to a `tracing-subscriber` filter for more detail:

```bash
# Linux or macOS
OXIDEGRAM_LOG=debug cargo run

# PowerShell
$env:OXIDEGRAM_LOG = "debug"
cargo run
```

## Development

OxideGram follows a DDD-inspired hexagonal structure. Dependency flow points inward, keeping Telegram, Lua, persistence, and terminal details outside the business rules:

```text
src/
|-- domain/          # Pure session and message-filtering rules
|-- application/     # Authentication use cases and dependency ports
|-- infrastructure/  # Telegram, Lua, crypto, files, and logging adapters
|-- presentation/    # Interactive terminal adapter
`-- main.rs          # Composition root and process lifecycle

presentation + infrastructure -> application -> domain
```

The CLI uses an `inquire` render theme designed for dark terminals. It does not override the terminal background. Set the standard `NO_COLOR` environment variable to disable prompt colors.

Run the standard checks before submitting changes:

```bash
cargo fmt -- --check
cargo check
cargo test
cargo clippy --all-targets -- -D warnings
```

The GitHub Pages documentation is stored in `docs/` and deployed by `.github/workflows/pages.yml`. In the GitHub repository settings, select **GitHub Actions** as the Pages source to enable deployment.

## Security Notes

- Session payloads in `data/config.oxide` are encrypted with AES-256-GCM using a key derived from your password with Argon2id.
- A working SQLite session file is recreated in `data/sessions/` during login. Protect the entire `data/` directory and the host account that runs OxideGram.
- Passwords, API hashes, and session payloads are not intentionally written to application logs.
- Lua scripts execute locally with the capabilities exposed by the embedded runtime. Only run scripts you trust.

## License

No license file has been added yet. Until a license is provided, the source remains under its default copyright restrictions.
