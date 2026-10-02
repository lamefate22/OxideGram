---
layout: default
title: Script Examples
---

# Script Examples

[Home](index.html) | [API Reference](api.html) | [Message Filters](filters.html)

This collection provides copy-ready, production-grade script examples for common Telegram automation patterns. Place these scripts in `data/bots/<name>.lua` or generate starting boilerplates with `oxidegram template create`.

---

## Echo Bot with Text Formatting

Replies to incoming private messages with formatted Markdown quotes:

```lua
ox.on_message({
    incoming = true,
    private = true,
    has_text = true,
}, function(event)
    -- Skip slash commands
    if event.text:starts_with("/") then
        return
    end

    local reply_content = string.format("Echo: *%s*", event.text)
    event:reply(reply_content, {
        parse_mode = "markdown",
        delay = 0.5,
    })
end)
```

---

## Interactive Keyboard Auto-Clicker

Simulates button interactions in third-party bots (e.g. captchas, daily check-ins, confirmations) via MTProto `GetBotCallbackAnswer` and reply keyboard automation:

```lua
ox.log:info("Keyboard automation handler active")

ox.on_message({ incoming = true }, function(event)
    -- Verify message contains keyboard buttons
    if #event.buttons == 0 then
        return
    end

    ox.log:info(string.format("Detected %d keyboard row(s) in message %d", #event.buttons, event.id))

    for row_idx, row in ipairs(event.buttons) do
        for col_idx, btn in ipairs(row) do
            ox.log:debug(string.format(
                "Button [%d,%d]: '%s' [type: %s, callback: %s]",
                row_idx, col_idx, btn.text, btn.type, tostring(btn.is_callback)
            ))

            -- Match verification or confirmation prompts
            local text_lower = btn.text:lower()
            if text_lower:contains("verify") or text_lower:contains("confirm") or text_lower:contains("подтвердить") then
                ox.log:info("Matching button detected. Clicking: " .. btn.text)
                
                -- Simulate click with a 1.0s humanized delay
                local result = event:click(btn.text, 1.0)
                ox.log:info("Callback answer received: " .. tostring(result))
                return
            end
        end
    end
end)
```

---

## Inline Callback Automation (JSON & Custom Payloads)

Many third-party Telegram bots (dating bots, captcha services, interactive games) use raw callback data payloads — such as JSON strings `{"com":"START_DIAL_POST","data":"1108360"}` or custom action identifiers `cb:vote:42`. 

OxideGram lets you trigger these callbacks directly via MTProto `GetBotCallbackAnswer` without posting visible text to the chat:

```lua
ox.log:info("Callback automation bot initialized")

-- Pattern 1: Inspect button callback payloads and click by data
ox.on_message({ incoming = true }, function(event)
    for _, row in ipairs(event.buttons) do
        for _, btn in ipairs(row) do
            if btn.is_callback and btn.callback_data then
                -- Check for structured JSON command in button payload
                if btn.callback_data:contains("START_DIAL_POST") then
                    ox.log:info("Target dial callback found, clicking button...")
                    -- Send callback answer with 0.5s delay
                    event:click_button(btn.callback_data, 0.5)
                    return
                end
            end
        end
    end
end)

-- Pattern 2: Click with raw JSON or callback string directly
ox.on_message({ incoming = true, pattern = "Поиск партн[её]ра" }, function(event)
    ox.log:info("Partner search prompt detected. Triggering callback query...")

    -- Direct click with JSON payload
    event:click_button('{"com":"START_DIAL_POST","data":"1108360"}', 1.0)
end)

-- Pattern 3: Standalone callback click on any message across chats
function trigger_remote_action(target_chat_id, target_msg_id)
    ox.click_button {
        chat_id = target_chat_id,
        message_id = target_msg_id,
        data = '{"com":"START_DIAL_POST","data":"1108360"}',
    }
end
```

---

## Animated Progress and Status Updater

Demonstrates message editing, asynchronous non-blocking delays, and final reaction feedback:

```lua
ox.on_message({
    commands = "deploy",
    outgoing = true,
}, function(event)
    local stages = {
        "⏳ [1/4] Preparing environment...",
        "📦 [2/4] Downloading assets...",
        "🔨 [3/4] Building packages...",
        "🚀 [4/4] Finalizing deployment...",
    }

    for _, stage in ipairs(stages) do
        event:edit(stage)
        ox.sleep_random(0.8, 1.4)
    end

    event:edit("✅ *Deployment complete!* All services operational.", {
        parse_mode = "markdown",
    })
    event:react("🎉", 0.5)
end)
```

---

## Regular Expression Parsing and Captures

Demonstrates Rust-compatible regular expressions with named capture groups and typing action indicators:

```lua
ox.on_message({
    pattern = "^/calc\\s+(?<a>\\d+)\\s*(?<op>[+\\-*/])\\s*(?<b>\\d+)$",
}, function(event)
    local a = tonumber(event.captures.a)
    local b = tonumber(event.captures.b)
    local op = event.captures.op

    -- Display typing action indicator before answering
    ox.send_typing(event.chat_id)
    ox.sleep(0.6)

    local result
    if op == "+" then
        result = a + b
    elseif op == "-" then
        result = a - b
    elseif op == "*" then
        result = a * b
    elseif op == "/" then
        if b == 0 then
            event:reply("❌ *Error:* Division by zero is undefined.", { parse_mode = "markdown" })
            return
        end
        result = a / b
    end

    local response = string.format("Calculation: `%d %s %d = %s`", a, op, b, tostring(result))
    event:reply(response, { parse_mode = "markdown" })
end)
```

---

## Periodic Background Tasks and Heartbeat

Demonstrates non-blocking background intervals, cancellation tokens, and storage state tracking:

```lua
local check_counter = 0

-- Schedule recurring health check every 60 seconds
local heartbeat_id = ox.set_interval(60.0, function()
    check_counter = check_counter + 1
    ox.log:info(string.format("Heartbeat #%d: Engine active and responsive", check_counter))
end)

-- Command to stop the recurring task
ox.on_message({ commands = "stopheartbeat", outgoing = true }, function(event)
    local stopped = ox.clear_interval(heartbeat_id)
    if stopped then
        event:reply("Heartbeat task successfully cancelled.")
    else
        event:reply("Timer was already inactive.")
    end
end)
```

---

## Media File Transfers

Demonstrates sending photos, uncompressed documents, and native voice notes:

```lua
ox.on_message({
    commands = "sendmedia",
    incoming = true,
}, function(event)
    -- 1. Send status photo
    ox.send_image(event.chat_id, "data/chart.png", "*Weekly Analytics Summary*", {
        parse_mode = "markdown",
        delay = 0.5,
    })

    -- 2. Send detailed PDF report
    ox.send_document(event.chat_id, "data/report.pdf", "Detailed metrics export", {
        delay = 1.0,
    })

    -- 3. Send audio voice greeting
    ox.send_voice(event.chat_id, "data/voice_note.ogg", nil, 1.5)
end)
```

---

## Multi-Step Conversation Flow and State Management

Demonstrates multi-turn state machines (`ox.flow`), command routing (`flow:command`), session data persistence (`ctx:set`/`ctx:get`), step timeouts, and global cancel handlers:

```lua
-- Declare flow restricted to private chats with 60-second idle step timeout
local dialog = ox.flow("onboarding_flow", {
    private = true,
    timeout = 60,
})

-- Top-level slash command router
dialog:command("start", function(event, ctx)
    event:reply("Welcome! What is your username or callsign?")
    return "ask_username"
end)

-- Step 1: Collect username
dialog:step("ask_username", {
    action = function(event, ctx)
        local username = event.text:trim()
        ctx:set("username", username)

        event:reply(string.format("Got it, %s! Please choose your notification frequency: [daily/weekly]", username))
        return "choose_frequency"
    end,
    on_timeout = function(ctx)
        ox.log:warn("Dialog step timed out waiting for username.")
    end,
})

-- Step 2: Choose frequency
dialog:step("choose_frequency", {
    match = { "daily", "weekly", "ежедневно", "еженедельно" },
    action = function(event, ctx)
        local freq = event.text:lower():trim()
        local username = ctx:get("username", "Unknown")

        -- Save to persistent storage across restarts
        ox.storage.set("user_" .. tostring(event.sender_id), {
            username = username,
            frequency = freq,
        })

        event:reply(string.format("✅ Configuration complete!\nUsername: `%s`\nFrequency: `%s`", username, freq), {
            parse_mode = "markdown",
        })
        ctx:reset()
    end,
})

-- Global cancellation trigger (works at any stage of the dialog)
dialog:on({ "cancel", "стоп", "отмена", "/cancel" }, function(event, ctx)
    ctx:reset()
    event:reply("Dialog session cancelled. State has been reset.")
end)
```

---

## Interactive Startup Configuration and File Prompt

Demonstrates configuring bot operational parameters in the terminal before connecting to Telegram, including file selection with autocomplete and validation:

```lua
-- 1. Choice menu with arrow navigation and default selection
local target_mode = ox.select("Select operational mode:", {
    { label = "Echo Mode (Standard replies)", value = "echo" },
    { label = "Broadcast Mode (Forwarding alerts)", value = "broadcast" },
    { label = "Monitor Mode (Silent logging)", value = "monitor" },
}, "echo")

-- 2. Boolean confirmation prompt
local auto_react = ox.confirm("Enable automatic emoji reactions?", true)

-- 3. File path prompt with tab-autocomplete and extension validation
local config_file = ox.file("Path to custom dataset:", {
    default = "data/dataset.json",
    extensions = { "json", "toml" },
    must_exist = true,
})

ox.log:info(string.format("Config loaded: mode=%s, auto_react=%s, file=%s", target_mode, tostring(auto_react), config_file))

ox.on_message({ incoming = true }, function(event)
    if auto_react then
        event:react("👀", 0.3)
    end

    if target_mode == "echo" then
        event:reply("Dataset in use: " .. config_file)
    end
end)
```

---

## Named Arguments and String Utilities

Demonstrates using named table arguments and enhanced string manipulation methods:

```lua
ox.on_message({ incoming = true }, function(event)
    local raw = event.text:trim()
    
    -- Check if empty or whitespace only
    if raw:is_blank() then
        return
    end

    -- Extract slash command and remainder arguments
    local cmd, args = raw:extract_command("mybot")
    if cmd == "shout" then
        -- Pad and escape text
        local clean_args = ox.escape_markdown(args)
        local formatted = clean_args:pad_left(clean_args:len() + 2, "🔊 ")

        -- Named table syntax
        event:reply {
            text = formatted,
            parse_mode = "markdown",
            delay = 0.5,
        }
    elseif cmd == "replace" then
        -- Substring replacement with regex support
        local masked = args:replace("\\d+", "###", true)
        event:reply { text = "Masked: " .. masked }
    end
end)
```

---

## Hot-Reload Workflow

OxideGram monitors script files on disk and automatically hot-reloads code without interrupting active MTProto sessions:

1. Launch your bot via the CLI:
   ```bash
   cargo run -- my_bot
   ```
2. Open `data/bots/my_bot.lua` in your editor (e.g. Zed or VS Code).
3. Modify existing handler logic or add a new command.
4. Save the file (`Ctrl+S`).
5. OxideGram instantly logs:
   ```text
   2026-10-02T10:15:12.123Z INFO oxidegram::infrastructure::lua::runner: Script change detected. Initiating hot-reload...
   2026-10-02T10:15:12.145Z INFO oxidegram::infrastructure::lua::runner: Lua script hot-reloaded successfully
   ```
6. The updated script logic takes effect immediately without reconnecting or re-authenticating with Telegram.
