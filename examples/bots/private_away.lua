-- Restrict this example with chats or senders before testing on a busy account.

ox.on_message({
    incoming = true,
    private = true,
    has_text = true
}, function(event)
    ox.send_message(event.chat_id, "I am currently away. I will reply later.", 1)
end)
