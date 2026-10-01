---
layout: default
title: Script Examples
---

# Script Examples

[Documentation home](index.html) | [API](api.html) | [Filters](filters.html)

Copy-ready versions of these scripts can be generated directly through OxideGram's interactive menu (`3. Create Bot Script Template`) or placed inside `data/bots/`.

---

## 1. Echo with Markdown Formatting

```lua
ox.on_message({ incoming = true, private = true, has_text = true }, function(event)
    if not event.text:find("^/") then
        local reply = string.format("Echo: *%s*", event.text)
        event.reply(reply, { parse_mode = "markdown", delay = 0.5 })
    end
end)
```

---

## 2. Interactive Buttons & Auto-Click Simulator

Simulates user button clicks in third-party bots (e.g. captchas, verifications, confirmations) via MTProto `GetBotCallbackAnswer`:

```lua
ox.log.info("Buttons simulator started")

ox.on_message({ incoming = true }, function(event)
    -- Check if message has inline or reply buttons
    if #event.buttons > 0 then
        ox.log.info(string.format("Detected %d rows of buttons", #event.buttons))

        for row_idx, row in ipairs(event.buttons) do
            for col_idx, btn in ipairs(row) do
                ox.log.debug(string.format("Row %d, Col %d: '%s' [%s]", row_idx, col_idx, btn.text, btn.type))

                -- Automatically click button matching 'Verify' or 'Confirm'
                if btn.text:lower():find("verify") or btn.text:lower():find("confirm") then
                    ox.log.info("Clicking button: " .. btn.text)
                    local answer = event.click(btn.text, 1.0)
                    ox.log.info("Telegram callback answer: " .. tostring(answer))
                    return
                end
            end
        end
    end
end)
```

---

## 3. Self-Editing Animated Message

Demonstrates editing messages and asynchronous pauses:

```lua
ox.on_message({ commands = "countdown", outgoing = true }, function(event)
    event.edit("Countdown: 3...")
    ox.sleep(1.0)
    event.edit("Countdown: 2...")
    ox.sleep(1.0)
    event.edit("Countdown: 1...")
    ox.sleep(1.0)
    event.edit("🚀 *LIFTOFF!*", { parse_mode = "markdown" })
end)
```

---

## 4. Regex Named Captures & Typing Status

Demonstrates regex named capture groups and human-like typing simulation:

```lua
ox.on_message({ pattern = "^/calc\\s+(?<a>\\d+)\\s*(?<op>[+\\-*/])\\s*(?<b>\\d+)$" }, function(event)
    local a = tonumber(event.captures.a)
    local b = tonumber(event.captures.b)
    local op = event.captures.op

    ox.send_typing(event.chat_id)
    ox.sleep(0.8)

    local res
    if op == "+" then res = a + b
    elseif op == "-" then res = a - b
    elseif op == "*" then res = a * b
    elseif op == "/" then res = (b ~= 0) and (a / b) or "Cannot divide by zero"
    end

    event.reply(string.format("Result: `%s`", tostring(res)), { parse_mode = "markdown" })
end)
```

---

## 5. Periodic Heartbeat Timer & Reactions

Runs background interval timers without blocking message reception:

```lua
-- Ping a log monitor every 60 seconds
local timer_id = ox.set_interval(60.0, function()
    ox.log.info("Periodic health check heartbeat")
end)

-- React with fire emoji to incoming mentions
ox.on_message({ pattern = "(?i)\\boxidegram\\b", incoming = true }, function(event)
    event.react("🔥")
    event.reply("Thanks for mentioning OxideGram!", { delay = 0.5 })
end)
```

---

## 6. Document & Voice Note Uploader

```lua
ox.on_message({ commands = "sendreport", incoming = true }, function(event)
    ox.send_document(event.chat_id, "data/report.pdf", "Here is your requested report")
    ox.send_voice(event.chat_id, "data/voice_note.ogg", 1.0)
end)
```

---

## 7. Hot-Reload Workflow

1. Start OxideGram and select your bot.
2. Open `data/bots/your_bot.lua` in VS Code or any text editor.
3. Edit your handlers or add new ones.
4. Hit **Save** (`Ctrl+S`).
5. OxideGram instantly logs:
   ```
   Script change detected. Initiating hot-reload...
   Lua script hot-reloaded successfully
   ```
6. The bot is immediately running the updated code without restarting or logging into Telegram again!
