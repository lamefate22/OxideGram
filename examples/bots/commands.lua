-- Respond to two common commands. Command matching is case-insensitive and
-- supports Telegram bot suffixes such as /start@example_bot.

ox.on_message({ incoming = true, commands = "start" }, function(event)
    ox.send_message(event.chat_id, "Welcome. Try /help.", 0)
end)

ox.on_message({ incoming = true, commands = "help" }, function(event)
    ox.send_message(event.chat_id, "Available commands: /start, /help", 0)
end)
