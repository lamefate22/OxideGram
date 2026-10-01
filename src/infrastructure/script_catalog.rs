//! Bot script loader scanning `data/bots/` directory for executable `.lua` scripts.

use crate::domain::automation::BotScript;
use crate::errors::{OxideError, ScriptError};
use crate::infrastructure::IO_TIMEOUT;
use std::path::{Path, PathBuf};
use tokio::fs;
use tracing::info;

/// Scanner for dynamic bot script discovery.
pub struct FileSystemScriptCatalog {
    pub bots_dir: PathBuf,
}

impl Default for FileSystemScriptCatalog {
    fn default() -> Self {
        Self::new("data/bots")
    }
}

impl FileSystemScriptCatalog {
    /// Creates a catalog pointing to the specified directory.
    pub fn new<P: AsRef<Path>>(path: P) -> Self {
        Self {
            bots_dir: path.as_ref().to_path_buf(),
        }
    }

    /// Ensures that the bot scripts directory exists on disk.
    pub async fn ensure_bots_dir_exists(&self) -> Result<(), OxideError> {
        if !self.bots_dir.exists() {
            tokio::time::timeout(IO_TIMEOUT, fs::create_dir_all(&self.bots_dir))
                .await
                .map_err(|_| ScriptError::ScriptIo {
                    path: self.bots_dir.clone(),
                    source: std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        "Create bots directory timed out",
                    ),
                })?
                .map_err(|source| ScriptError::ScriptIo {
                    path: self.bots_dir.clone(),
                    source,
                })?;
        }
        Ok(())
    }

    /// Searches the `data/bots` directory for available `.lua` bot scripts.
    pub async fn search_bots(&self) -> Result<Vec<BotScript>, OxideError> {
        self.ensure_bots_dir_exists().await?;
        let mut discovered = Vec::new();

        let mut entries = tokio::time::timeout(IO_TIMEOUT, fs::read_dir(&self.bots_dir))
            .await
            .map_err(|_| ScriptError::ScriptIo {
                path: self.bots_dir.clone(),
                source: std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "Read bots directory timed out",
                ),
            })?
            .map_err(|source| ScriptError::ScriptIo {
                path: self.bots_dir.clone(),
                source,
            })?;

        while let Some(entry) = tokio::time::timeout(IO_TIMEOUT, entries.next_entry())
            .await
            .map_err(|_| ScriptError::ScriptIo {
                path: self.bots_dir.clone(),
                source: std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "Read next bot entry timed out",
                ),
            })?
            .map_err(|source| ScriptError::ScriptIo {
                path: self.bots_dir.clone(),
                source,
            })?
        {
            let path = entry.path();
            if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("lua") {
                let stem = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown_bot");

                discovered.push(BotScript {
                    name: stem.to_string(),
                    path,
                });
            }
        }

        Ok(discovered)
    }

    /// Creates a new bot template script in the bots directory.
    pub async fn create_bot_template(
        &self,
        name: &str,
        template_kind: &str,
    ) -> Result<PathBuf, OxideError> {
        self.ensure_bots_dir_exists().await?;
        let sanitized = name.trim().replace(['/', '\\', ' '], "_");
        let file_name = if sanitized.ends_with(".lua") {
            sanitized
        } else {
            format!("{sanitized}.lua")
        };
        let target_path = self.bots_dir.join(&file_name);

        if target_path.exists() {
            return Err(ScriptError::Runtime(format!(
                "Bot script already exists: {}",
                target_path.display()
            ))
            .into());
        }

        let content = match template_kind {
            "buttons" => TEMPLATE_BUTTONS,
            "full" => TEMPLATE_FULL,
            "flow" => TEMPLATE_FLOW,
            _ => TEMPLATE_ECHO,
        };

        tokio::time::timeout(IO_TIMEOUT, fs::write(&target_path, content))
            .await
            .map_err(|_| ScriptError::ScriptIo {
                path: target_path.clone(),
                source: std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "Write bot template timed out",
                ),
            })?
            .map_err(|source| ScriptError::ScriptIo {
                path: target_path.clone(),
                source,
            })?;

        info!(path = %target_path.display(), template_kind, "Created bot script template");
        Ok(target_path)
    }
}

const TEMPLATE_ECHO: &str = r#"-- OxideGram Bot: Echo & Commands Template
-- Automatic Hot-Reload is active: edit and save this file to reload immediately!

ox.log.info("Echo Bot loaded successfully")

-- Handle /start command
ox.on_message({ commands = { "start" } }, function(event)
    event.reply("Hello from OxideGram userbot! Type /ping to test me.")
end)

-- Handle /ping command with optional delay
ox.on_message({ commands = { "ping" } }, function(event)
    event.react("🏓")
    event.reply("Pong! Latency is nominal.", { delay = 0.5 })
end)

-- Echo private incoming messages
ox.on_message({ private = true, incoming = true }, function(event)
    if not event.text:find("^/") then
        event.reply("You said: " .. event.text)
    end
end)
"#;

const TEMPLATE_BUTTONS: &str = r#"-- OxideGram Bot: Buttons & Clicks Simulator Template
-- This bot demonstrates parsing inline/reply buttons and simulating clicks.

ox.log.info("Buttons Bot loaded")

-- Inspect incoming message for inline buttons
ox.on_message({ incoming = true }, function(event)
    if #event.buttons > 0 then
        ox.log.info(string.format("Found message with %d button rows", #event.buttons))

        -- Find button labeled "Confirm" or "Verify" and automatically click it
        for _, row in ipairs(event.buttons) do
            for _, btn in ipairs(row) do
                ox.log.debug("Button: " .. btn.text .. " [type: " .. btn.type .. "]")
                if btn.text:lower():find("verify") or btn.text:lower():find("confirm") then
                    ox.log.info("Clicking verification button: " .. btn.text)
                    local result = event.click(btn.text, 1.0)
                    ox.log.info("Callback answer: " .. tostring(result))
                    return
                end
            end
        end
    end
end)

-- Command to test reacting and pin
ox.on_message({ commands = { "pinthis" } }, function(event)
    event.react("📌")
    event.pin()
    event.reply("Message pinned successfully!")
end)
"#;

const TEMPLATE_FULL: &str = r#"-- OxideGram Bot: Full Capabilities Showcase Template
-- Demonstrates Regex captures, timers, rich text formatting, and flood protection.

ox.log.info("Full Showcase Bot initialized")

-- Background periodic timer (e.g. heartbeat or cache cleanup)
local timer_id = ox.set_interval(60.0, function()
    ox.log.info("Periodic health check tick")
end)

-- Regex filter with named captures: /weather <city>
ox.on_message({ pattern = "^/weather\\s+(?<city>[a-zA-Z\\-]+)$" }, function(event)
    local city = event.captures.city or event.matches[1] or "Unknown"
    event.react("🌤")
    ox.send_typing(event.chat_id)

    -- Markdown formatted response
    local report = string.format("*Weather for %s*:\n• Temperature: `+21°C`\n• Conditions: _Clear sky_", city)
    event.reply(report, { parse_mode = "markdown", delay = 0.8 })
end)

-- Self-editing message demo: /count
ox.on_message({ commands = { "count" }, outgoing = true }, function(event)
    event.edit("Counting: 3...")
    ox.sleep(1.0)
    event.edit("Counting: 2...")
    ox.sleep(1.0)
    event.edit("Counting: 1...")
    ox.sleep(1.0)
    event.edit("🚀 *Blast off!*", { parse_mode = "markdown" })
end)
"#;

const TEMPLATE_FLOW: &str = r#"-- OxideGram Bot: Dialog Flow & State Machine Template
-- Demonstrates ox.flow, ox.storage, and string methods.
-- Test offline: cargo run -- test-bot <path_to_this_file>

local TARGET_CHAT = 6015596466 -- Replace with your target chat ID or nil

-- Load persistent counter from ox.storage
local session_count = ox.storage.get("session_count", 0) + 1
ox.storage.set("session_count", session_count)

-- Create a dialog flow (FSM)
local dialog = ox.flow("dating_bot", {
    target_chat = TARGET_CHAT,
    timeout = 45, -- step timeout in seconds
})

-- Initialize custom data in flow
dialog.data.grade = "💋Поцелуй"
dialog.data.message = "Привет! Как твои дела?"

-- Step 1: Wait for menu prompt
dialog:step("menu", {
    match = "Меню:",
    action = function(event, ctx)
        -- String methods like event.text:contains(...) are available on all strings
        event.click("💋 или 👋", 1)
        return "card"
    end
})

-- Step 2: Rate or text message
dialog:step("card", {
    match = "💋 или 👋",
    action = function(event, ctx)
        event.click("💌 Сообщение")
        return "input_text"
    end
})

-- Step 3: Send message
dialog:step("input_text", {
    match = "✍️Введите",
    action = function(event, ctx)
        ox.send_message(event.chat_id, ctx.data.message, 1.0)
        return "menu"
    end
})

-- Global matchers (e.g. stop triggers)
dialog:on_match("На сегодня много поцелуйчиков", function(event)
    ox.stop()
end)
"#;

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDirGuard(PathBuf);

    impl TempDirGuard {
        fn new() -> Self {
            let path =
                std::env::temp_dir().join(format!("oxidegram_bots_{}", rand::random::<u64>()));
            Self(path)
        }
    }

    impl Drop for TempDirGuard {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[tokio::test]
    async fn test_empty_catalog_returns_no_scripts() {
        let temp = TempDirGuard::new();
        let catalog = FileSystemScriptCatalog::new(&temp.0);
        let bots = catalog.search_bots().await.unwrap();
        assert!(bots.is_empty());
    }

    #[tokio::test]
    async fn test_filters_only_lua_files_and_ignores_subdirs() {
        let temp = TempDirGuard::new();
        let catalog = FileSystemScriptCatalog::new(&temp.0);
        catalog.ensure_bots_dir_exists().await.unwrap();

        // Create valid lua files
        std::fs::write(temp.0.join("bot1.lua"), "print('bot1')").unwrap();
        std::fs::write(temp.0.join("bot2.lua"), "print('bot2')").unwrap();

        // Create non-lua files
        std::fs::write(temp.0.join("readme.txt"), "info").unwrap();
        std::fs::write(temp.0.join("config.json"), "{}").unwrap();

        // Create a subdirectory with a lua file inside
        let sub = temp.0.join("subdir");
        std::fs::create_dir(&sub).unwrap();
        std::fs::write(sub.join("nested.lua"), "print('nested')").unwrap();

        let mut bots = catalog.search_bots().await.unwrap();
        bots.sort_by(|a, b| a.name.cmp(&b.name));

        assert_eq!(bots.len(), 2);
        assert_eq!(bots[0].name, "bot1");
        assert_eq!(bots[1].name, "bot2");
    }

    #[tokio::test]
    async fn test_creates_and_loads_templates_properly() {
        let temp = TempDirGuard::new();
        let catalog = FileSystemScriptCatalog::new(&temp.0);

        let echo_path = catalog
            .create_bot_template("my_echo", "echo")
            .await
            .unwrap();
        assert!(echo_path.exists());
        let echo_content = std::fs::read_to_string(&echo_path).unwrap();
        assert!(echo_content.contains("Echo & Commands"));

        let buttons_path = catalog
            .create_bot_template("my_buttons", "buttons")
            .await
            .unwrap();
        assert!(buttons_path.exists());
        let buttons_content = std::fs::read_to_string(&buttons_path).unwrap();
        assert!(buttons_content.contains("Buttons & Clicks"));

        let full_path = catalog
            .create_bot_template("my_full", "full")
            .await
            .unwrap();
        assert!(full_path.exists());
        let full_content = std::fs::read_to_string(&full_path).unwrap();
        assert!(full_content.contains("Showcase"));

        let flow_path = catalog
            .create_bot_template("my_flow", "flow")
            .await
            .unwrap();
        assert!(flow_path.exists());
        let flow_content = std::fs::read_to_string(&flow_path).unwrap();
        assert!(flow_content.contains("ox.flow"));

        // Duplicate creation must fail
        assert!(
            catalog
                .create_bot_template("my_echo", "echo")
                .await
                .is_err()
        );

        let bots = catalog.search_bots().await.unwrap();
        assert_eq!(bots.len(), 4);
    }
}
