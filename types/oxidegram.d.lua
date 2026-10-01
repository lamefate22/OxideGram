---@meta OxideGram
--- OxideGram Lua Scripting Engine Type Definitions
--- Compatible with Lua Language Server (LuaLS), EmmyLua, VS Code, and Neovim.

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
---@param text string Message text content.
---@param options? MessageOptions|number Options table or numeric delay in seconds.
---@return nil
function MessageEvent.reply(text, options) end

--- Edits this message with new text.
---@param new_text string New message text.
---@param options? MessageOptions|number Options table or numeric delay in seconds.
---@return nil
function MessageEvent.edit(new_text, options) end

--- Deletes this message.
---@param delay? number Optional delay in seconds before deleting.
---@return nil
function MessageEvent.delete(delay) end

--- Sends an emoji reaction to this message.
---@param emoji string Reaction emoji (e.g. "👍", "🔥", "❤️").
---@param delay? number Optional delay in seconds.
---@return nil
function MessageEvent.react(emoji, delay) end

--- Pins this message in the chat.
---@param delay? number Optional delay in seconds.
---@return nil
function MessageEvent.pin(delay) end

--- Clicks an inline button or simulates pressing a regular reply keyboard button.
---
--- Supports exact text, case-insensitive substring, emoji match, or 1-based numeric index.
--- If the button is an inline callback button, executes MTProto `GetBotCallbackAnswer`.
--- If the button is a reply keyboard button, sends the button label text to the chat.
---@param query_or_index string|integer Button label text or 1-based index (e.g. `1` for the first button).
---@param delay? number Optional delay in seconds.
---@return BotCallbackAnswer|boolean Result of clicking.
function MessageEvent.click(query_or_index, delay) end

--- Alias for `event.click`.
---@param query_or_index string|integer
---@param delay? number
---@return BotCallbackAnswer|boolean
function MessageEvent.click_button(query_or_index, delay) end

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
---@field private? boolean Match 1-on-1 private user chats.
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
---@generic T
---@param key string Storage key.
---@param default? T Default value returned if key does not exist.
---@return T
function StorageAPI.get(key, default) end

--- Stores a value in persistent storage (persisted to disk as JSON).
---@param key string Storage key.
---@param value any Value to store (string, number, boolean, or table).
---@return nil
function StorageAPI.set(key, value) end

--- Checks if a key exists in storage.
---@param key string Storage key.
---@return boolean
function StorageAPI.has(key) end

--- Deletes a key from storage.
---@param key string Storage key.
---@return boolean `true` if key was deleted, `false` if it didn't exist.
function StorageAPI.delete(key) end

--- Returns all stored key-value pairs as a Lua table.
---@return table<string, any>
function StorageAPI.all() end

--- Clears all stored data for this bot.
---@return nil
function StorageAPI.clear() end

--------------------------------------------------------------------------------
-- 5. Logging API (`ox.log`)
--------------------------------------------------------------------------------

--- Structured logging API emitting compact console logs and detailed file logs.
---@class LogAPI
local LogAPI = {}

--- Logs an informational message.
---@param message string
---@return nil
function LogAPI.info(message) end

--- Logs a warning message.
---@param message string
---@return nil
function LogAPI.warn(message) end

--- Logs an error message.
---@param message string
---@return nil
function LogAPI.error(message) end

--- Logs a debug message (written to file log or verbose console).
---@param message string
---@return nil
function LogAPI.debug(message) end

--------------------------------------------------------------------------------
-- 6. Dialog Flow FSM API (`ox.flow`)
--------------------------------------------------------------------------------

--- Configuration options for a dialog flow state machine.
---@class FlowOptions
---@field target_chat? integer Optional target chat ID to restrict this flow to.
---@field timeout? number Timeout in seconds before the current step resets.

--- Context passed to flow step action callbacks.
---@class FlowContext
---@field data table<string, any> Shared state data preserved across transitions.
---@field current_step string Name of the currently active step.

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
---@param name string Name of the step (e.g. "ask_grade", "confirm").
---@param config StepConfig Step matching rules and action handler.
---@return Flow
function Flow:step(name, config) end

--- Transitions the flow to a specific step immediately.
---@param step_name string Target step name.
---@return Flow
function Flow:go_to(step_name) end

--- Resets the flow to its initial / idle state.
---@return Flow
function Flow:reset() end

--- Registers a global pattern matching regardless of the active step.
---@param pattern_or_table string|string[] Substring or regex.
---@param action fun(event: MessageEvent, ctx: FlowContext): string|nil
---@return Flow
function Flow:on_match(pattern_or_table, action) end

--------------------------------------------------------------------------------
-- 7. Global `ox` Engine Table
--------------------------------------------------------------------------------

--- OxideGram Global Automation API.
---@class OxideGramAPI
---@field storage StorageAPI Persistent key-value storage.
---@field log LogAPI Logging utilities.
ox = {}

--- Registers a message event listener with optional filter predicates.
---@param filter_or_callback MessageFilter|fun(event: MessageEvent) Filter table or callback function.
---@param callback? fun(event: MessageEvent) Callback executed when incoming message matches filter.
---@return nil
function ox.on_message(filter_or_callback, callback) end

--- Sends a text message to the specified Telegram chat.
--- Automatically handles FloodWait rate-limiting with exponential backoff.
---@param chat_id integer Peer chat ID.
---@param text string Message text.
---@param options? MessageOptions|number Options table or numeric delay in seconds.
---@param extra_delay? number Additional delay in seconds.
---@return nil
function ox.send_message(chat_id, text, options, extra_delay) end

--- Alias for `ox.send_message`.
---@param chat_id integer
---@param text string
---@param options? MessageOptions|number
---@return nil
function ox.reply(chat_id, text, options) end

--- Clicks an inline callback button by peer chat, message ID, and button data string.
---@param chat_id integer Peer chat ID.
---@param message_id integer ID of message containing the inline button.
---@param data string Binary callback payload data.
---@return BotCallbackAnswer
function ox.click_button(chat_id, message_id, data) end

--- Edits an existing message sent by your userbot.
---@param chat_id integer Peer chat ID.
---@param message_id integer ID of message to edit.
---@param new_text string New message text.
---@param options? MessageOptions|number Options table or numeric delay in seconds.
---@return nil
function ox.edit_message(chat_id, message_id, new_text, options) end

--- Deletes a single message by ID.
---@param chat_id integer Peer chat ID.
---@param message_id integer ID of message to delete.
---@param delay? number Delay in seconds before deleting.
---@return nil
function ox.delete_message(chat_id, message_id, delay) end

--- Deletes multiple messages in a chat.
---@param chat_id integer Peer chat ID.
---@param message_ids integer[] Array of numeric message IDs.
---@param delay? number Delay in seconds before deleting.
---@return nil
function ox.delete_messages(chat_id, message_ids, delay) end

--- Sends an emoji reaction to a message.
---@param chat_id integer Peer chat ID.
---@param message_id integer Message ID to react to.
---@param emoji string Emoji string (e.g. "👍", "🔥", "❤️").
---@param delay? number Delay in seconds.
---@return nil
function ox.send_reaction(chat_id, message_id, emoji, delay) end

--- Alias for `ox.send_reaction`.
---@param chat_id integer
---@param message_id integer
---@param emoji string
---@param delay? number
---@return nil
function ox.react(chat_id, message_id, emoji, delay) end

--- Pins a message in a chat.
---@param chat_id integer Peer chat ID.
---@param message_id integer Message ID to pin.
---@param delay? number Delay in seconds.
---@return nil
function ox.pin_message(chat_id, message_id, delay) end

--- Forwards a message from one dialog to another.
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

--- Sends a photo to a chat.
---@param chat_id integer Peer chat ID.
---@param path string File system path to the image file.
---@param caption? string Optional caption text.
---@param options? MessageOptions|number
---@return nil
function ox.send_photo(chat_id, path, caption, options) end

--- Sends a document or generic file to a chat.
---@param chat_id integer Peer chat ID.
---@param path string File system path to the document.
---@param caption? string Optional caption text.
---@param options? MessageOptions|number
---@return nil
function ox.send_document(chat_id, path, caption, options) end

--- Sends an audio track to a chat.
---@param chat_id integer Peer chat ID.
---@param path string File system path to the audio file.
---@param caption? string Optional caption text.
---@param options? MessageOptions|number
---@return nil
function ox.send_audio(chat_id, path, caption, options) end

--- Sends a voice message to a chat.
---@param chat_id integer Peer chat ID.
---@param path string File system path to the voice file (e.g. .ogg/.opus).
---@param caption? string Optional caption text.
---@param options? MessageOptions|number
---@return nil
function ox.send_voice(chat_id, path, caption, options) end

--- Schedules a recurring callback timer.
---@param duration_sec number Interval duration in seconds.
---@param callback fun() Function called on every tick.
---@return integer timer_id Timer handle used to cancel via `ox.clear_interval`.
function ox.set_interval(duration_sec, callback) end

--- Cancels an active interval timer.
---@param timer_id integer ID returned by `ox.set_interval`.
---@return boolean `true` if cancelled, `false` if not found.
function ox.clear_interval(timer_id) end

--- Schedules a one-shot timeout callback.
---@param duration_sec number Delay duration in seconds before running.
---@param callback fun() Function called after delay.
---@return integer timer_id Timer handle used to cancel via `ox.clear_timeout`.
function ox.set_timeout(duration_sec, callback) end

--- Cancels an active timeout timer.
---@param timer_id integer ID returned by `ox.set_timeout`.
---@return boolean `true` if cancelled, `false` if not found.
function ox.clear_timeout(timer_id) end

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

--- Prompts the user in the terminal during startup configuration.
---@param prompt string Prompt label shown to the user.
---@param default? string Default value returned on empty input.
---@return string User entered string.
function ox.input(prompt, default) end

--- Creates or retrieves a named Dialog Flow state machine.
---@param name string Unique name of the flow.
---@param options? FlowOptions Flow configuration.
---@return Flow
function ox.flow(name, options) end

--- Gracefully requests the bot to stop its event loop and terminate.
---@return nil
function ox.stop() end

--------------------------------------------------------------------------------
-- 8. String Standard Library Extensions
--------------------------------------------------------------------------------

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

--- Global shortcut for `string:contains`.
---@param str string
---@param substring string
---@return boolean
function contains(str, substring) end

--- Global shortcut for `string:starts_with`.
---@param str string
---@param prefix string
---@return boolean
function starts_with(str, prefix) end

--- Global shortcut for `string:ends_with`.
---@param str string
---@param suffix string
---@return boolean
function ends_with(str, suffix) end

--- Global shortcut for `string:split`.
---@param str string
---@param separator? string
---@return string[]
function split(str, separator) end

--- Global shortcut for `string:trim`.
---@param str string
---@return string
function trim(str) end
