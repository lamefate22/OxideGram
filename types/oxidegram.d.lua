---@meta
--- OxideGram Lua Scripting Engine Type Definitions
--- Compatible with Lua Language Server (LuaLS), EmmyLua, VS Code, and Zed.

--------------------------------------------------------------------------------
-- 1. General & Options Types
--------------------------------------------------------------------------------

---@alias ParseMode "markdown" | "md" | "html"

--- Options for sending, replying, and editing messages.
---@class MessageOptions
---@field delay? number Delay in seconds to wait before sending (applies human-like jitter).
---@field parse_mode? ParseMode Text formatting mode ("markdown" or "html").

--- Callback answer received after clicking an inline callback button.
---@class BotCallbackAnswer
---@field alert boolean Whether an alert banner was shown.
---@field message? string Toast or alert message text returned by the bot.
---@field url? string Destination URL if the button redirected to an external link.

--- Inline or Reply keyboard button representation.
---@class Button
---@field text string The display label or emoji on the button.
---@field type "callback" | "url" | "text" | "other" Type of button.
---@field data? string Binary callback payload data (for inline buttons).
---@field url? string Destination link (for URL buttons).

--------------------------------------------------------------------------------
-- 2. Message Event
--------------------------------------------------------------------------------

--- Options table for event:reply.
---@class EventReplyTableOptions : MessageOptions
---@field text string Message content to send.

--- Options table for event:edit.
---@class EventEditTableOptions : MessageOptions
---@field text string New message content.

--- Options table for event:react.
---@class EventReactTableOptions
---@field reaction? string Emoji to react with.
---@field emoji? string Alias for reaction.
---@field delay? number Optional delay in seconds.

--- Options table for event:click.
---@class EventClickTableOptions
---@field query? string Button text or substring to match.
---@field text? string Alias for query.
---@field data? string Raw callback data payload (e.g. JSON or callback string).
---@field index? integer 1-based index of button.
---@field delay? number Optional delay in seconds.

--- Options table for event:pin.
---@class EventPinTableOptions
---@field delay? number Optional delay in seconds.

--- Options table for event:delete.
---@class EventDeleteTableOptions
---@field delay? number Optional delay in seconds.

--- Enriched Telegram message event passed to message callbacks and flow actions.
---@class MessageEvent
---@field text string Content of the message.
---@field id integer Numeric unique identifier of the message.
---@field message_id integer Alias for `event.id`.
---@field chat_id integer Identifier of the chat / dialog.
---@field sender_id integer Peer identifier of the message sender (0 if anonymous).
---@field incoming boolean `true` if the message was sent by someone else.
---@field outgoing boolean `true` if the message was sent from your account.
---@field is_private boolean `true` if the message is in a 1-on-1 private chat.
---@field is_group boolean `true` if the message is in a standard basic group.
---@field is_channel boolean `true` if the message is in a channel or supergroup.
---@field matches string[] Positional regex capture groups (`matches[1]`, `matches[2]`).
---@field captures table<string, string> Named regex capture groups (`captures["name"]`).
---@field buttons Button[][] 2D matrix of keyboard buttons attached to this message or active in chat.
local MessageEvent = {}

--- Replies directly to this message with a quote (`reply_to`).
--- Supports both positional syntax and named table syntax.
--- Supports both colon syntax (`event:reply(...)`) and dot syntax (`event.reply(...)`).
---
--- Example:
--- ```lua
--- event:reply("Hello!")
--- event.reply("Hello!")
--- event:reply { text = "Hello with delay", delay = 1.0 }
--- event.reply { text = "Hello with delay", delay = 1.0 }
--- ```
---@overload fun(options: EventReplyTableOptions): nil
---@overload fun(text: string, options?: MessageOptions|number): nil
---@overload fun(self: MessageEvent, options: EventReplyTableOptions): nil
---@param text string Message text content.
---@param options? MessageOptions|number Options table or numeric delay in seconds.
---@return nil
function MessageEvent:reply(text, options) end

--- Edits this message with new text.
--- Supports both positional syntax and named table syntax.
--- Supports both colon syntax (`event:edit(...)`) and dot syntax (`event.edit(...)`).
---
--- Example:
--- ```lua
--- event:edit("Updated text")
--- event.edit("Updated text")
--- event:edit { text = "Updated with mode", parse_mode = "markdown" }
--- event.edit { text = "Updated with mode", parse_mode = "markdown" }
--- ```
---@overload fun(options: EventEditTableOptions): nil
---@overload fun(new_text: string, options?: MessageOptions|number): nil
---@overload fun(self: MessageEvent, options: EventEditTableOptions): nil
---@param new_text string New message text.
---@param options? MessageOptions|number Options table or numeric delay in seconds.
---@return nil
function MessageEvent:edit(new_text, options) end

--- Deletes this message.
--- Supports both colon syntax (`event:delete(...)`) and dot syntax (`event.delete(...)`).
---
--- Example:
--- ```lua
--- event:delete()
--- event.delete()
--- event:delete(2.0)
--- event.delete(2.0)
--- event:delete { delay = 2.0 }
--- event.delete { delay = 2.0 }
--- ```
---@overload fun(self: MessageEvent, options?: EventDeleteTableOptions|number): nil
---@param delay_or_options? number|EventDeleteTableOptions Optional delay in seconds or options table.
---@return nil
function MessageEvent.delete(delay_or_options) end

--- Sends an emoji reaction to this message.
--- Supports both positional syntax and named table syntax.
--- Supports both colon syntax (`event:react(...)`) and dot syntax (`event.react(...)`).
---
--- Example:
--- ```lua
--- event:react("👍")
--- event.react("👍")
--- event:react { emoji = "🔥", delay = 0.5 }
--- event.react { emoji = "🔥", delay = 0.5 }
--- ```
---@overload fun(options: EventReactTableOptions): nil
---@overload fun(emoji: string, delay?: number): nil
---@overload fun(self: MessageEvent, options: EventReactTableOptions): nil
---@param emoji string Reaction emoji (e.g. "👍", "🔥", "❤️").
---@param delay? number Optional delay in seconds.
---@return nil
function MessageEvent:react(emoji, delay) end

--- Pins this message in the chat.
--- Supports both colon syntax (`event:pin(...)`) and dot syntax (`event.pin(...)`).
---
--- Example:
--- ```lua
--- event:pin()
--- event.pin()
--- event:pin { delay = 0.5 }
--- event.pin { delay = 0.5 }
--- ```
---@overload fun(self: MessageEvent, options?: EventPinTableOptions|number): nil
---@param delay_or_options? number|EventPinTableOptions Optional delay in seconds or options table.
---@return nil
function MessageEvent.pin(delay_or_options) end

--- Clicks an inline button or simulates pressing a regular reply keyboard button.
---
--- Supports exact text, case-insensitive substring, emoji match, or 1-based numeric index.
--- If the button is an inline callback button, executes MTProto `GetBotCallbackAnswer`.
--- If the button is a reply keyboard button, sends the button label text to the chat.
--- Supports both colon syntax (`event:click(...)`) and dot syntax (`event.click(...)`).
---
--- Example:
--- ```lua
--- event:click("Next")
--- event.click("Next")
--- event:click(1)
--- event.click(1)
--- event:click { query = "Next", delay = 1.0 }
--- event.click { query = "Next", delay = 1.0 }
--- ```
---@overload fun(options: EventClickTableOptions): BotCallbackAnswer|boolean
---@overload fun(query_or_index: string|integer, delay?: number): BotCallbackAnswer|boolean
---@overload fun(self: MessageEvent, options: EventClickTableOptions): BotCallbackAnswer|boolean
---@param query_or_index string|integer Button label text or 1-based index (e.g. `1` for the first button).
---@param delay? number Optional delay in seconds.
---@return BotCallbackAnswer|boolean Result of clicking.
function MessageEvent:click(query_or_index, delay) end

--- Direct alias for clicking an inline callback button using raw callback data payload.
--- Executes MTProto `GetBotCallbackAnswer`.
---@overload fun(options: EventClickTableOptions): BotCallbackAnswer|boolean
---@overload fun(callback_data: string, delay?: number): BotCallbackAnswer|boolean
---@overload fun(self: MessageEvent, options: EventClickTableOptions): BotCallbackAnswer|boolean
---@param callback_data string Raw callback data payload (e.g. `'{"com":"START_DIAL_POST"}'`).
---@param delay? number Optional delay in seconds.
---@return BotCallbackAnswer|boolean Result of clicking.
function MessageEvent:click_button(callback_data, delay) end

--------------------------------------------------------------------------------
-- 3. Message Filter Specification
--------------------------------------------------------------------------------

--- Predicates used to filter incoming or outgoing Telegram messages.
--- All specified fields must match (logical AND).
---@class MessageFilter
---@field chats? integer|integer[] Dialog ID or list of dialog IDs to accept.
---@field senders? integer|integer[] Sender ID or list of sender IDs to accept.
---@field incoming? boolean Match only incoming messages (`true`) or outgoing (`false`).
---@field outgoing? boolean Match only outgoing messages (`true`) or incoming (`false`).
---@field ["private"]? boolean Match 1-on-1 private user chats.
---@field group? boolean Match basic groups.
---@field channel? boolean Match channels and supergroups.
---@field has_text? boolean `true` if text is non-empty, `false` if empty.
---@field commands? string|string[] Slash command name or array of commands without slash (e.g. `"start"` or `{"ping", "status"}`).
---@field pattern? string Rust regular expression pattern to match against `event.text`.

--------------------------------------------------------------------------------
-- 4. Persistent Key-Value Storage API (`ox.storage`)
--------------------------------------------------------------------------------

--- Persistent JSON storage saved to `data/storage/<bot_name>.json`.
--- Automatically preserved and loaded across bot restarts.
---@class StorageAPI
local StorageAPI = {}

--- Retrieves a value from persistent storage, or default if missing.
--- Supports both dot syntax (`ox.storage.get(...)`) and colon syntax (`ox.storage:get(...)`).
---@generic T
---@overload fun(self: StorageAPI, key: string, default?: T): T
---@param key string Storage key.
---@param default? T Default value returned if key does not exist.
---@return T
function StorageAPI.get(key, default) end

--- Stores a value in persistent storage (persisted to disk as JSON).
--- Supports both dot syntax (`ox.storage.set(...)`) and colon syntax (`ox.storage:set(...)`).
---@overload fun(self: StorageAPI, key: string, value: any): nil
---@param key string Storage key.
---@param value any Value to store (string, number, boolean, or table).
---@return nil
function StorageAPI.set(key, value) end

--- Checks if a key exists in storage.
--- Supports both dot syntax (`ox.storage.has(...)`) and colon syntax (`ox.storage:has(...)`).
---@overload fun(self: StorageAPI, key: string): boolean
---@param key string Storage key.
---@return boolean
function StorageAPI.has(key) end

--- Deletes a key from storage.
--- Supports both dot syntax (`ox.storage.delete(...)`) and colon syntax (`ox.storage:delete(...)`).
---@overload fun(self: StorageAPI, key: string): boolean
---@param key string Storage key.
---@return boolean `true` if key was deleted, `false` if it didn't exist.
function StorageAPI.delete(key) end

--- Returns all stored key-value pairs as a Lua table.
--- Supports both dot syntax (`ox.storage.all()`) and colon syntax (`ox.storage:all()`).
---@overload fun(self: StorageAPI): table<string, any>
---@return table<string, any>
function StorageAPI.all() end

--- Clears all stored data for this bot.
--- Supports both dot syntax (`ox.storage.clear()`) and colon syntax (`ox.storage:clear()`).
---@overload fun(self: StorageAPI): nil
---@return nil
function StorageAPI.clear() end

--------------------------------------------------------------------------------
-- 5. Logging API (`ox.log`)
--------------------------------------------------------------------------------

--- Structured logging API emitting compact console logs and detailed file logs.
---@class LogAPI
local LogAPI = {}

--- Logs an informational message.
--- Supports both dot syntax (`ox.log.info(...)`) and colon syntax (`ox.log:info(...)`).
---@overload fun(self: LogAPI, message: any): nil
---@param message any
---@return nil
function LogAPI.info(message) end

--- Logs a warning message.
--- Supports both dot syntax (`ox.log.warn(...)`) and colon syntax (`ox.log:warn(...)`).
---@overload fun(self: LogAPI, message: any): nil
---@param message any
---@return nil
function LogAPI.warn(message) end

--- Logs an error message.
--- Supports both dot syntax (`ox.log.error(...)`) and colon syntax (`ox.log:error(...)`).
---@overload fun(self: LogAPI, message: any): nil
---@param message any
---@return nil
function LogAPI.error(message) end

--- Logs a debug message (written to file log or verbose console).
--- Supports both dot syntax (`ox.log.debug(...)`) and colon syntax (`ox.log:debug(...)`).
---@overload fun(self: LogAPI, message: any): nil
---@param message any
---@return nil
function LogAPI.debug(message) end

--------------------------------------------------------------------------------
-- 6. Dialog Flow FSM API (`ox.flow`)
--------------------------------------------------------------------------------

--- Configuration options for a dialog flow state machine.
---@class FlowOptions
---@field target_chat? integer Optional target chat ID to restrict this flow to.
---@field chats? integer|integer[] Dialog ID or list of dialog IDs to accept.
---@field senders? integer|integer[] Sender ID or list of sender IDs to accept.
---@field incoming? boolean Match only incoming messages (`true`) or outgoing (`false`).
---@field outgoing? boolean Match only outgoing messages (`true`) or incoming (`false`).
---@field ["private"]? boolean Match 1-on-1 private user chats.
---@field group? boolean Match basic groups.
---@field channel? boolean Match channels and supergroups.
---@field has_text? boolean `true` if text is non-empty, `false` if empty.
---@field timeout? number Timeout in seconds before the current step resets.

--- Context passed to flow step action callbacks.
---@class FlowContext
---@field data table<string, any> Shared state data preserved across transitions.
---@field current_step string Name of the currently active step.
local FlowContext = {}

--- Sets a key in this flow's session state.
--- Supports both colon syntax (`ctx:set(...)`) and dot syntax (`ctx.set(...)`).
---@overload fun(key: string, value: any): nil
---@param key string
---@param value any
---@return nil
function FlowContext:set(key, value) end

--- Gets a value from this flow's session state.
--- Supports both colon syntax (`ctx:get(...)`) and dot syntax (`ctx.get(...)`).
---@generic T
---@overload fun(key: string, default?: T): T
---@param key string
---@param default? T
---@return T
function FlowContext:get(key, default) end

--- Step definition configuration table.
---@class StepConfig
---@field match? string|string[] Substring or list of substrings to match in `event.text`.
---@field commands? string|string[] Command or list of commands (e.g. `"start"`).
---@field pattern? string Regex pattern.
---@field action fun(event: MessageEvent, ctx: FlowContext): string|nil Handler function. Returning a step name transitions to that step.

--- Dialog Flow State Machine instance.
---@class Flow
---@field data table<string, any> Shared key-value state for this flow.
local Flow = {}

--- Defines a named step in the state machine.
--- Supports both colon syntax (`flow:step(...)`) and dot syntax (`flow.step(...)`).
---@overload fun(name: string, config: StepConfig): Flow
---@param name string Name of the step (e.g. "ask_grade", "confirm").
---@param config StepConfig Step matching rules and action handler.
---@return Flow
function Flow:step(name, config) end

--- Registers a top-level command router (replaces `ox.on_message` for slash commands).
--- Supports both colon syntax (`flow:command(...)`) and dot syntax (`flow.command(...)`).
---
--- Example:
--- ```lua
--- flow:command("start", function(event, ctx)
---     event:reply("Welcome! Enter your name:")
---     return "ask_name"
--- end)
--- ```
---@overload fun(commands: string|string[], action: fun(event: MessageEvent, ctx: FlowContext): string|nil): Flow
---@param commands string|string[] Command name or list of command names without slash.
---@param action fun(event: MessageEvent, ctx: FlowContext): string|nil
---@return Flow
function Flow:command(commands, action) end

--- Registers a top-level pattern / keyword trigger.
--- Supports both colon syntax (`flow:on(...)`) and dot syntax (`flow.on(...)`).
---
--- Example:
--- ```lua
--- flow:on("help", function(event, ctx)
---     event:reply("Available commands: /start, /cancel")
--- end)
--- ```
---@overload fun(pattern_or_keywords: string|string[], action: fun(event: MessageEvent, ctx: FlowContext): string|nil): Flow
---@param pattern_or_keywords string|string[] Substring or regex pattern.
---@param action fun(event: MessageEvent, ctx: FlowContext): string|nil
---@return Flow
function Flow:on(pattern_or_keywords, action) end

--- Transitions the flow to a specific step immediately.
--- Supports both colon syntax (`flow:go_to(...)`) and dot syntax (`flow.go_to(...)`).
---@overload fun(step_name: string): Flow
---@param step_name string Target step name.
---@return Flow
function Flow:go_to(step_name) end

--- Resets the flow to its initial / idle state.
--- Supports both colon syntax (`flow:reset()`) and dot syntax (`flow.reset()`).
---@overload fun(): Flow
---@return Flow
function Flow:reset() end

--------------------------------------------------------------------------------
-- 7. Global `ox` Engine Table
--------------------------------------------------------------------------------

--- Options table for `ox.send_message`.
---@class SendMessageTableOptions : MessageOptions
---@field chat_id integer Target chat ID.
---@field text string Message text content.

--- Options table for `ox.edit_message`.
---@class EditMessageTableOptions : MessageOptions
---@field chat_id integer Target chat ID.
---@field message_id integer ID of message to edit.
---@field text string New message content.

--- Options table for `ox.delete_message`.
---@class DeleteMessageTableOptions
---@field chat_id integer Target chat ID.
---@field message_id? integer Single message ID to delete.
---@field ids? integer[] Array of numeric message IDs to delete.
---@field delay? number Optional delay in seconds.

--- Options table for `ox.react`.
---@class ReactTableOptions
---@field chat_id integer Target chat ID.
---@field message_id integer Message ID to react to.
---@field emoji string Emoji string (e.g. "👍").
---@field reaction? string Alias for emoji.
---@field delay? number Optional delay in seconds.

--- Options table for `ox.pin_message`.
---@class PinMessageTableOptions
---@field chat_id integer Target chat ID.
---@field message_id integer Message ID to pin.
---@field delay? number Optional delay in seconds.

--- Options table for `ox.forward_message`.
---@class ForwardMessageTableOptions
---@field to_chat_id integer Destination chat ID.
---@field from_chat_id integer Source chat ID.
---@field message_id integer Message ID to forward.
---@field delay? number Optional delay in seconds.

--- Options table for media sending functions.
---@class SendMediaTableOptions : MessageOptions
---@field chat_id integer Target chat ID.
---@field path string File system path to the media file.
---@field caption? string Optional caption text.

--- Options table for `ox.click_button`.
---@class ClickButtonTableOptions
---@field chat_id integer Target chat ID.
---@field message_id integer ID of message containing the button.
---@field data string Button callback payload data.

--- Configuration options for `ox.file`.
---@class FileConfig
---@field default? string Default fallback path if user presses Enter without typing.
---@field must_exist? boolean Whether the file must exist on disk (default true).
---@field extensions? string[]|string Allowed file extensions without dot (e.g. `{"json", "txt"}` or `"json,txt"`).

--- Options table for `ox.file` when passed as a single table.
---@class FileTableOptions : FileConfig
---@field prompt string Prompt label shown to the user.

--- Options table for `ox.input` when passed as a single table.
---@class InputTableOptions
---@field prompt string Prompt label shown to the user.
---@field default? string Default fallback value.

--- Options table for `ox.select` when passed as a single table.
---@class SelectTableOptions
---@field prompt string Prompt label shown to the user.
---@field options (string | SelectOptionItem)[] Array of options or label/value tables.
---@field default? string Default selected option.

--- Options table for `ox.confirm` when passed as a single table.
---@class ConfirmTableOptions
---@field prompt string Question or prompt text.
---@field default? boolean Default boolean answer when pressing Enter.

--- OxideGram Global Automation API.
---@class OxideGramAPI
---@field storage StorageAPI Persistent key-value storage.
---@field log LogAPI Logging utilities.
---@field string StringAPI String helper functions.
ox = {}

--- Registers a message event listener with optional filter predicates.
---@param filter_or_callback MessageFilter|fun(event: MessageEvent) Filter table or callback function.
---@param callback? fun(event: MessageEvent) Callback executed when incoming message matches filter.
---@return nil
function ox.on_message(filter_or_callback, callback) end

--- Sends a text message to the specified Telegram chat.
--- Automatically handles FloodWait rate-limiting with exponential backoff.
--- Supports both positional syntax and named table syntax.
---
--- Example:
--- ```lua
--- ox.send_message(123456, "Hello world!")
--- ox.send_message { chat_id = 123456, text = "Hello!", delay = 1.0 }
--- ```
---@overload fun(options: SendMessageTableOptions): nil
---@param chat_id integer Peer chat ID.
---@param text string Message text.
---@param options? MessageOptions|number Options table or numeric delay in seconds.
---@return nil
function ox.send_message(chat_id, text, options) end

--- Clicks an inline callback button by peer chat, message ID, and button data string.
--- Supports both positional syntax and named table syntax.
---
--- Example:
--- ```lua
--- ox.click_button(chat_id, message_id, "confirm_payment")
--- ox.click_button { chat_id = chat_id, message_id = message_id, data = "confirm_payment" }
--- ```
---@overload fun(options: ClickButtonTableOptions): BotCallbackAnswer
---@param chat_id integer Peer chat ID.
---@param message_id integer ID of message containing the inline button.
---@param data string Binary callback payload data.
---@return BotCallbackAnswer
function ox.click_button(chat_id, message_id, data) end

--- Edits an existing message sent by your userbot.
--- Supports both positional syntax and named table syntax.
---
--- Example:
--- ```lua
--- ox.edit_message(chat_id, msg_id, "New text")
--- ox.edit_message { chat_id = chat_id, message_id = msg_id, text = "New text" }
--- ```
---@overload fun(options: EditMessageTableOptions): nil
---@param chat_id integer Peer chat ID.
---@param message_id integer ID of message to edit.
---@param new_text string New message text.
---@param options? MessageOptions|number Options table or numeric delay in seconds.
---@return nil
function ox.edit_message(chat_id, message_id, new_text, options) end

--- Deletes a single message or multiple messages by ID.
--- Supports both positional syntax and named table syntax.
---
--- Example:
--- ```lua
--- ox.delete_message(chat_id, msg_id)
--- ox.delete_message(chat_id, { 101, 102, 103 })
--- ox.delete_message { chat_id = chat_id, message_id = msg_id, delay = 1.0 }
--- ox.delete_message { chat_id = chat_id, ids = { 101, 102 }, delay = 1.0 }
--- ```
---@overload fun(options: DeleteMessageTableOptions): nil
---@param chat_id integer Peer chat ID.
---@param message_id_or_ids integer|integer[] Message ID or array of message IDs to delete.
---@param delay? number Delay in seconds before deleting.
---@return nil
function ox.delete_message(chat_id, message_id_or_ids, delay) end

--- Sends an emoji reaction to a message.
--- Supports both positional syntax and named table syntax.
---
--- Example:
--- ```lua
--- ox.react(chat_id, msg_id, "👍")
--- ox.react { chat_id = chat_id, message_id = msg_id, emoji = "🔥" }
--- ```
---@overload fun(options: ReactTableOptions): nil
---@param chat_id integer Peer chat ID.
---@param message_id integer Message ID to react to.
---@param emoji string Emoji string (e.g. "👍", "🔥", "❤️").
---@param delay? number Delay in seconds.
---@return nil
function ox.react(chat_id, message_id, emoji, delay) end

--- Pins a message in a chat.
--- Supports both positional syntax and named table syntax.
---
--- Example:
--- ```lua
--- ox.pin_message(chat_id, msg_id)
--- ox.pin_message { chat_id = chat_id, message_id = msg_id }
--- ```
---@overload fun(options: PinMessageTableOptions): nil
---@param chat_id integer Peer chat ID.
---@param message_id integer Message ID to pin.
---@param delay? number Delay in seconds.
---@return nil
function ox.pin_message(chat_id, message_id, delay) end

--- Forwards a message from one dialog to another.
--- Supports both positional syntax and named table syntax.
---
--- Example:
--- ```lua
--- ox.forward_message(target_chat, source_chat, msg_id)
--- ox.forward_message { to_chat_id = target_chat, from_chat_id = source_chat, message_id = msg_id }
--- ```
---@overload fun(options: ForwardMessageTableOptions): nil
---@param to_chat_id integer Destination chat ID.
---@param from_chat_id integer Source chat ID.
---@param message_id integer ID of message to forward.
---@param delay? number Delay in seconds.
---@return nil
function ox.forward_message(to_chat_id, from_chat_id, message_id, delay) end

--- Simulates chat action (typing, upload document, etc.).
---@param chat_id integer Peer chat ID.
---@param action? "typing" | "cancel" | "record_video" | "upload_video" | "record_voice" | "upload_voice" | "upload_document" | "choose_sticker" Action string (default "typing").
---@return nil
function ox.send_typing(chat_id, action) end

--- Sends an image to a chat.
--- Supports both positional syntax and named table syntax.
---
--- Example:
--- ```lua
--- ox.send_image(chat_id, "data/photo.jpg", "Photo caption")
--- ox.send_image { chat_id = chat_id, path = "data/photo.jpg", caption = "Caption" }
--- ```
---@overload fun(options: SendMediaTableOptions): nil
---@param chat_id integer Peer chat ID.
---@param path string File system path to the image file.
---@param caption? string Optional caption text.
---@param options? MessageOptions|number
---@return nil
function ox.send_image(chat_id, path, caption, options) end

--- Sends a document or generic file to a chat.
--- Supports both positional syntax and named table syntax.
---
--- Example:
--- ```lua
--- ox.send_document(chat_id, "data/report.pdf", "Monthly report")
--- ox.send_document { chat_id = chat_id, path = "data/report.pdf" }
--- ```
---@overload fun(options: SendMediaTableOptions): nil
---@param chat_id integer Peer chat ID.
---@param path string File system path to the document.
---@param caption? string Optional caption text.
---@param options? MessageOptions|number
---@return nil
function ox.send_document(chat_id, path, caption, options) end

--- Sends an audio track to a chat.
--- Supports both positional syntax and named table syntax.
---
--- Example:
--- ```lua
--- ox.send_audio(chat_id, "audio/track.mp3", "Track title")
--- ox.send_audio { chat_id = chat_id, path = "audio/track.mp3" }
--- ```
---@overload fun(options: SendMediaTableOptions): nil
---@param chat_id integer Peer chat ID.
---@param path string File system path to the audio file.
---@param caption? string Optional caption text.
---@param options? MessageOptions|number
---@return nil
function ox.send_audio(chat_id, path, caption, options) end

--- Sends a voice message to a chat.
--- Supports both positional syntax and named table syntax.
---
--- Example:
--- ```lua
--- ox.send_voice(chat_id, "audio/voice.ogg")
--- ox.send_voice { chat_id = chat_id, path = "audio/voice.ogg" }
--- ```
---@overload fun(options: SendMediaTableOptions): nil
---@param chat_id integer Peer chat ID.
---@param path string File system path to the voice file (e.g. .ogg/.opus).
---@param caption? string Optional caption text.
---@param options? MessageOptions|number
---@return nil
function ox.send_voice(chat_id, path, caption, options) end

--- Schedules a recurring callback timer.
---@param duration_sec number Interval duration in seconds.
---@param callback fun() Function called on every tick.
---@return integer timer_id Timer handle used to cancel via `ox.clear_interval` or `ox.clear_timer`.
function ox.set_interval(duration_sec, callback) end

--- Cancels an active interval timer.
---@param timer_id integer ID returned by `ox.set_interval`.
---@return boolean `true` if cancelled, `false` if not found.
function ox.clear_interval(timer_id) end

--- Schedules a one-shot timeout callback.
---@param duration_sec number Delay duration in seconds before running.
---@param callback fun() Function called after delay.
---@return integer timer_id Timer handle used to cancel via `ox.clear_timeout` or `ox.clear_timer`.
function ox.set_timeout(duration_sec, callback) end

--- Cancels an active timeout timer.
---@param timer_id integer ID returned by `ox.set_timeout`.
---@return boolean `true` if cancelled, `false` if not found.
function ox.clear_timeout(timer_id) end

--- Cancels an active timer (interval or timeout).
---@param timer_id integer ID returned by `ox.set_interval` or `ox.set_timeout`.
---@return boolean `true` if cancelled, `false` if not found.
function ox.clear_timer(timer_id) end

--- Pauses execution for the specified number of seconds without blocking the runtime.
---@param seconds number Sleep duration in seconds.
---@return nil
function ox.sleep(seconds) end

--- Sleeps for a random duration between `min` and `max` seconds (humanized delay).
---@param min number Minimum sleep in seconds.
---@param max number Maximum sleep in seconds.
---@return nil
function ox.sleep_random(min, max) end

--- Returns a random element from a Lua array table.
---@generic T
---@param list T[] Array of elements.
---@return T
function ox.choice(list) end

--- Single select option item with optional display label and underlying value.
---@class SelectOptionItem
---@field label string Display label shown in the terminal menu.
---@field value? string Value returned when chosen (defaults to label).

--- Prompts the user with an interactive terminal selection menu (via arrow keys or search).
--- Supports simple string arrays `{ "text", "grades" }`, detailed `{ { label = "Text", value = "text" } }`, or named table syntax.
---
--- Example:
--- ```lua
--- local mode = ox.select("Desired spam mode?", { "text", "grades" }, "text")
--- local mode = ox.select { prompt = "Desired spam mode?", options = { "text", "grades" }, default = "text" }
--- ```
---@overload fun(options: SelectTableOptions): string
---@param prompt string Prompt label shown to the user.
---@param options (string | SelectOptionItem)[] Array of options or label/value tables.
---@param default? string Default selected option.
---@return string The chosen option value.
function ox.select(prompt, options, default) end

--- Prompts the user for a boolean yes/no confirmation.
--- Supports both positional syntax and named table syntax.
---
--- Example:
--- ```lua
--- local delete_sent = ox.confirm("Delete sent messages?", true)
--- local delete_sent = ox.confirm { prompt = "Delete sent messages?", default = true }
--- ```
---@overload fun(options: ConfirmTableOptions): boolean
---@param prompt string Question or prompt text.
---@param default? boolean Default boolean answer when pressing Enter.
---@return boolean
function ox.confirm(prompt, default) end

--- Prompts the user for a text string in the terminal during startup configuration.
--- Supports both positional syntax and named table syntax.
---
--- Example:
--- ```lua
--- local key = ox.input("Enter API key:")
--- local mode = ox.input("Desired mode:", "text")
--- local text = ox.input { prompt = "Enter prompt:", default = "hello" }
--- ```
---@overload fun(options: InputTableOptions): string
---@param prompt string Prompt label shown to the user.
---@param default? string Default value string.
---@return string User entered string.
function ox.input(prompt, default) end

--- Prompts the user for a file path with tab-autocomplete and extension validation.
--- Supports both positional syntax and named table syntax.
---
--- Example:
--- ```lua
--- local file_path = ox.file("Path to config:", { default = "config.json", extensions = { "json" } })
--- local file_path = ox.file { prompt = "Path to file:", default = "data.csv" }
--- ```
---@overload fun(options: FileTableOptions): string
---@param prompt string Question or prompt text.
---@param options? FileConfig|string Optional configuration table or default path string.
---@return string The selected and validated file path.
function ox.file(prompt, options) end

--- Escapes special characters for Telegram MarkdownV2 formatting.
---@param text string
---@return string
function ox.escape_markdown(text) end

--- Escapes special characters (`&`, `<`, `>`, `"`) for Telegram HTML formatting.
---@param text string
---@return string
function ox.escape_html(text) end

--- Creates or retrieves a named Dialog Flow state machine.
---
--- Example:
--- ```lua
--- local flow = ox.flow("onboarding", { private = true })
--- flow:command("start", function(event, ctx)
---     event:reply("Welcome! What is your name?")
---     return "ask_name"
--- end)
--- ```
---@param name string Unique name of the flow.
---@param options? FlowOptions Flow configuration and chat filters.
---@return Flow
function ox.flow(name, options) end

--- Gracefully requests the bot to stop its event loop and terminate.
---@return nil
function ox.stop() end

--------------------------------------------------------------------------------
-- 8. String Standard Library Extensions (`ox.string` & Metatable)
--------------------------------------------------------------------------------

--- String helper utilities available via `ox.string.*` and string metatable methods (`str:method()`).
---@class StringAPI
---@field contains fun(str: string, substring: string): boolean
---@field starts_with fun(str: string, prefix: string): boolean
---@field ends_with fun(str: string, suffix: string): boolean
---@field split fun(str: string, separator?: string): string[]
---@field trim fun(str: string): string
---@field replace fun(str: string, target: string, replacement: string, is_regex?: boolean): string
---@field strip_prefix fun(str: string, prefix: string): string
---@field strip_suffix fun(str: string, suffix: string): string
---@field pad_left fun(str: string, length: integer, pad_char?: string): string
---@field pad_right fun(str: string, length: integer, pad_char?: string): string
---@field is_empty fun(str: string): boolean
---@field is_blank fun(str: string): boolean
---@field escape_markdown fun(str: string): string
---@field escape_html fun(str: string): string
---@field extract_command fun(str: string, bot_username?: string): string|nil, string
---@field lines fun(str: string): string[]

--- Checks if string contains a substring (case-sensitive or plain).
---@param self string
---@param substring string
---@return boolean
function string:contains(substring) end

--- Checks if string starts with a given prefix.
---@param self string
---@param prefix string
---@return boolean
function string:starts_with(prefix) end

--- Checks if string ends with a given suffix.
---@param self string
---@param suffix string
---@return boolean
function string:ends_with(suffix) end

--- Splits string by separator into an array table of substrings.
---@param self string
---@param separator? string Delimiter (default is whitespace).
---@return string[]
function string:split(separator) end

--- Strips leading and trailing whitespace from string.
---@param self string
---@return string
function string:trim() end

--- Replaces occurrences of `target` with `replacement`. If `is_regex` is true, compiles target as regular expression.
---@param self string
---@param target string Substring or regex pattern.
---@param replacement string Replacement text.
---@param is_regex? boolean Whether `target` is a regular expression (default false).
---@return string
function string:replace(target, replacement, is_regex) end

--- Strips prefix from string if it starts with it.
---@param self string
---@param prefix string
---@return string
function string:strip_prefix(prefix) end

--- Strips suffix from string if it ends with it.
---@param self string
---@param suffix string
---@return string
function string:strip_suffix(suffix) end

--- Pads string on the left until it reaches `length`.
---@param self string
---@param length integer
---@param pad_char? string Single padding character (default " ").
---@return string
function string:pad_left(length, pad_char) end

--- Pads string on the right until it reaches `length`.
---@param self string
---@param length integer
---@param pad_char? string Single padding character (default " ").
---@return string
function string:pad_right(length, pad_char) end

--- Returns true if string is empty (`""`).
---@param self string
---@return boolean
function string:is_empty() end

--- Returns true if string is empty or contains only whitespace.
---@param self string
---@return boolean
function string:is_blank() end

--- Escapes special characters for Telegram MarkdownV2 formatting.
---@param self string
---@return string
function string:escape_markdown() end

--- Escapes special characters (`&`, `<`, `>`, `"`) for Telegram HTML formatting.
---@param self string
---@return string
function string:escape_html() end

--- Extracts command name without slash and arguments from string.
---@param self string
---@param bot_username? string Bot username to strip (e.g. "mybot" for `/start@mybot`).
---@return string|nil command Command name or nil if not a command.
---@return string args Remainder arguments string.
function string:extract_command(bot_username) end

--- Splits string into array of lines.
---@param self string
---@return string[]
function string:lines() end

