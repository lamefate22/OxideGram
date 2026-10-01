---
layout: default
title: Script Examples
---

# Script Examples

[Home](index.html) | [API](api.html) | [Filters](filters.html)

This collection provides copy-ready, production-grade script examples for common Telegram automation patterns. Scripts can be placed in `data/bots/<name>.lua` or generated via the built-in template generator (`oxidegram template create`).

---

## 1. Echo Handler with Markdown Formatting

A minimal handler that replies to private incoming text messages with formatted Markdown:

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

## 2. Interactive Keyboard Auto-Clicker

Simulates button interactions in third-party bots (e.g. captcha verification, daily check-ins, confirmation dialogs) via MTProto `GetBotCallbackAnswer` and reply keyboard simulation:

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
            ox.log:debug(string.format("Button [%d,%d]: '%s' [type: %s]", row_idx, col_idx, btn.text, btn.type))

            -- Match verification or confirmation prompts
            local text_lower = btn.text:to_lower()
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

## 3. Animated Progress and Status Updater

Demonstrates message editing, asynchronous non-blocking pauses, and final reaction feedback:

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

## 4. Regex Matching with Positional and Named Captures

Demonstrates Rust-compatible regular expression parsing with named capture groups and typing indicators:

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

## 5. Periodic Heartbeat and Background Tasks

Demonstrates non-blocking background intervals, cancellation tokens, and storage state tracking:

```lua
local check_counter = 0

-- Schedule recurring health check every 60 seconds
local heartbeat_id = ox.set_interval(60.0, function()
    check_counter = check_counter + 1
    ox.log:info(string.format("Heartbeat #%d: Engine active and responsive", check_counter))
end)

-- Command to inspect and stop the recurring task
ox.on_message({ commands = "stopheartbeat", outgoing = true }, function(event)
    local stopped = ox.clear_timer(heartbeat_id)
    if stopped then
        event:reply("Heartbeat task successfully cancelled.")
    else
        event:reply("Timer was already inactive.")
    end
end)
```

---

## 6. Media File Transfers (Image, Document, Voice)

Demonstrates sending photos, uncompressed documents, and native voice notes with custom delays:

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
    ox.send_voice(event.chat_id, "data/voice_note.ogg", 1.5)
end)
```

---

## 7. Multi-Step Dialog State Machine with Persistent Storage

Demonstrates multi-turn state machines (`ox.flow`), step timeouts, session data retention (`ox.storage`), and interactive options:

```lua
-- Session counter persisted across bot restarts
local session_count = ox.storage.get("session_count", 0) + 1
ox.storage.set("session_count", session_count)
ox.log:info(string.format("Total bot sessions executed: %d", session_count))

-- Declare state machine with a 60-second idle step timeout
local dialog = ox.flow("onboarding_flow", {
    timeout = 60,
})

-- Shared memory preserved across all transitions
dialog.data.user_info = {}

-- Step 1: Greeting and trigger
dialog:step("start", {
    match = { "start", "привет", "hello" },
    commands = "start",
    action = function(event, ctx)
        event:reply("Welcome! What is your username or callsign?")
        return "ask_username"
    end,
})

-- Step 2: Receive username
dialog:step("ask_username", {
    action = function(event, ctx)
        local username = event.text:trim()
        ctx.data.user_info.username = username

        event:reply(string.format("Got it, %s! Please choose your preferred notification frequency: [daily/weekly]", username))
        return "choose_frequency"
    end,
    on_timeout = function(ctx)
        ox.log:warn("Dialog step timed out waiting for username. Resetting state.")
    end,
})

-- Step 3: Choose frequency
dialog:step("choose_frequency", {
    match = { "daily", "weekly", "ежедневно", "еженедельно" },
    action = function(event, ctx)
        local freq = event.text:to_lower():trim()
        ctx.data.user_info.frequency = freq

        -- Persist user configuration
        ox.storage.set("configured_user_" .. tostring(event.sender_id), ctx.data.user_info)

        event:reply(string.format("✅ Configuration complete!\nUsername: `%s`\nFrequency: `%s`", ctx.data.user_info.username, freq), {
            parse_mode = "markdown",
        })
        return "start" -- return to idle state
    end,
})

-- Global cancellation trigger (works at any stage of the dialog)
dialog:on_match({ "cancel", "стоп", "отмена", "/cancel" }, function(event)
    dialog:reset(event.chat_id)
    event:reply("Dialog session cancelled. State has been reset.")
end)
```

---

## 8. Interactive Startup Configuration

Demonstrates configuring bot operational parameters in the terminal before connecting to Telegram:

```lua
-- 1. Choice menu with arrow navigation and default selection
local target_mode = ox.select("Select operational mode:", {
    { label = "Echo Mode (Standard replies)", value = "echo" },
    { label = "Broadcast Mode (Forwarding alerts)", value = "broadcast" },
    { label = "Monitor Mode (Silent logging)", value = "monitor" },
}, "echo")

-- 2. Boolean confirmation prompt
local auto_react = ox.confirm("Enable automatic emoji reactions?", true)

-- 3. String input prompt with default value
local filter_tag = ox.input("Filter hashtag or keyword:", "#alerts")

ox.log:info(string.format("Configuration loaded: mode=%s, auto_react=%s, tag=%s", target_mode, tostring(auto_react), filter_tag))

ox.on_message({ incoming = true }, function(event)
    if not event.text:contains(filter_tag) then
        return
    end

    if auto_react then
        event:react("👀", 0.3)
    end

    if target_mode == "echo" then
        event:reply("Matched tag: " .. filter_tag)
    end
end)
```

---

## 9. Hot-Reload Workflow and Development Cycle

OxideGram monitors script files on disk and transparently hot-reloads code without interrupting active MTProto sessions:

1. Launch your bot via the CLI or runner:
   ```bash
   oxidegram run my_bot
   ```
2. Open `data/bots/my_bot.lua` in your preferred editor (e.g. Zed or Visual Studio Code).
3. Add a new command or modify an existing handler logic.
4. Save the file (`Ctrl+S`).
5. OxideGram instantly logs:
   ```text
   2026-10-01T20:30:12.123Z INFO oxidegram::infrastructure::lua::runner: Script change detected. Initiating hot-reload...
   2026-10-01T20:30:12.145Z INFO oxidegram::infrastructure::lua::runner: Lua script hot-reloaded successfully
   ```
6. The updated script logic takes effect immediately without reconnecting or re-authenticating with Telegram.
