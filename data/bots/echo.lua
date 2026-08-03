-- Example Echo Bot for OxideGram (Rust)
-- Filters support chats, senders, incoming, outgoing, private, group,
-- channel, has_text, commands and a Rust regular expression in pattern.

ox.on_message({ incoming = true, has_text = true }, function(event)
    local reply_text = "Hello! You said: '" .. event.text .. "'"
    ox.send_message(event.chat_id, reply_text, 0)
end)
