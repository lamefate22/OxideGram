-- Print event metadata to discover IDs and understand incoming updates.

ox.on_message(function(event)
    print("text:", event.text)
    print("chat_id:", event.chat_id)
    print("sender_id:", event.sender_id)
    print("incoming:", event.incoming)
    print("outgoing:", event.outgoing)
    print("private:", event.is_private)
    print("group:", event.is_group)
    print("channel:", event.is_channel)
end)
