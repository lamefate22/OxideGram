-- Rust regex syntax is used. Lua strings require escaped backslashes.

ox.on_message({
    incoming = true,
    has_text = true,
    pattern = "(?i)\\boxidegram\\b"
}, function(event)
    ox.send_message(event.chat_id, "OxideGram was mentioned.", 0)
end)
