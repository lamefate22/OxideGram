local ADMIN_ID = 123456789

ox.on_message({ senders = ADMIN_ID, incoming = true, commands = "stop" }, function(event)
    ox.send_message(event.chat_id, "Bot stopped", 0)
    ox.stop()
    return
end)
