use crate::domain::types::{ChatId, SenderId};
use regex::Regex;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BotScript {
    pub name: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatType {
    Private,
    Group,
    Channel,
}

/// The kind of interactive button in a Telegram keyboard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ButtonKind {
    /// Inline button with callback data triggered via `GetBotCallbackAnswer`.
    Callback(Vec<u8>),
    /// Inline button opening a URL.
    Url(String),
    /// Regular keyboard button that sends its text as a chat message.
    Text,
    /// Other button types (e.g. game, switch inline, web view).
    Other,
}

/// Representation of a single button in an inline or reply keyboard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BotButton {
    pub text: String,
    pub kind: ButtonKind,
}

impl BotButton {
    pub fn new(text: impl Into<String>, kind: ButtonKind) -> Self {
        Self {
            text: text.into(),
            kind,
        }
    }

    #[allow(dead_code)]
    pub fn is_callback(&self) -> bool {
        matches!(self.kind, ButtonKind::Callback(_))
    }

    pub fn callback_data(&self) -> Option<&[u8]> {
        match &self.kind {
            ButtonKind::Callback(data) => Some(data),
            _ => None,
        }
    }
}

/// A row of buttons in a Telegram keyboard.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct KeyboardRow {
    pub buttons: Vec<BotButton>,
}

/// Parsed message keyboard (inline or regular reply keyboard).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MessageMarkup {
    pub rows: Vec<KeyboardRow>,
    pub is_inline: bool,
}

impl MessageMarkup {
    /// Searches for a button matching text (case-insensitive) or raw callback_data string.
    pub fn find_button(&self, query: &str) -> Option<(usize, usize, &BotButton)> {
        for (r_idx, row) in self.rows.iter().enumerate() {
            for (c_idx, btn) in row.buttons.iter().enumerate() {
                if btn.text.eq_ignore_ascii_case(query) {
                    return Some((r_idx, c_idx, btn));
                }
                if let Some(data) = btn.callback_data()
                    && let Ok(data_str) = std::str::from_utf8(data)
                    && data_str == query
                {
                    return Some((r_idx, c_idx, btn));
                }
            }
        }
        None
    }

    /// Gets a button at 0-indexed row and column coordinates.
    #[allow(dead_code)]
    pub fn button_at(&self, row: usize, col: usize) -> Option<&BotButton> {
        self.rows.get(row).and_then(|r| r.buttons.get(col))
    }

    /// Gets a button at a 0-indexed flat index across all rows.
    pub fn flat_button_at(&self, mut index: usize) -> Option<&BotButton> {
        for row in &self.rows {
            if index < row.buttons.len() {
                return Some(&row.buttons[index]);
            }
            index -= row.buttons.len();
        }
        None
    }
}

#[derive(Debug)]
pub struct MessageContext<'a> {
    pub text: &'a str,
    pub chat_id: ChatId,
    pub sender_id: SenderId,
    pub incoming: bool,
    pub chat_type: ChatType,
}

#[derive(Debug, Clone, Default)]
pub struct MessageFilter {
    pub chats: Option<Vec<ChatId>>,
    pub senders: Option<Vec<SenderId>>,
    pub incoming: Option<bool>,
    pub outgoing: Option<bool>,
    pub private: Option<bool>,
    pub group: Option<bool>,
    pub channel: Option<bool>,
    pub has_text: Option<bool>,
    pub commands: Option<Vec<String>>,
    pub pattern: Option<Regex>,
}

/// Result of matching a message filter against a message context.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FilterMatchResult {
    /// Positional regex capture groups (matches[0] = first capture group, etc.).
    pub matches: Vec<String>,
    /// Named regex capture groups (?<name>...).
    pub captures: std::collections::HashMap<String, String>,
}

impl MessageFilter {
    /// Checks whether the message matches the filter rules.
    #[allow(dead_code)]
    pub fn matches(&self, context: &MessageContext<'_>) -> bool {
        self.match_context(context).is_some()
    }

    /// Evaluates filter rules and returns capture groups if matched.
    pub fn match_context(&self, context: &MessageContext<'_>) -> Option<FilterMatchResult> {
        if self
            .chats
            .as_ref()
            .is_some_and(|ids| !ids.contains(&context.chat_id))
            || self
                .senders
                .as_ref()
                .is_some_and(|ids| !ids.contains(&context.sender_id))
            || self
                .incoming
                .is_some_and(|expected| expected != context.incoming)
            || self
                .outgoing
                .is_some_and(|expected| expected != !context.incoming)
            || self
                .private
                .is_some_and(|expected| expected != (context.chat_type == ChatType::Private))
            || self
                .group
                .is_some_and(|expected| expected != (context.chat_type == ChatType::Group))
            || self
                .channel
                .is_some_and(|expected| expected != (context.chat_type == ChatType::Channel))
            || self
                .has_text
                .is_some_and(|expected| expected != !context.text.is_empty())
        {
            return None;
        }

        if let Some(commands) = &self.commands {
            let command = context
                .text
                .strip_prefix('/')
                .and_then(|text| text.split_whitespace().next())
                .and_then(|text| text.split('@').next());
            if !command.is_some_and(|cmd| {
                commands
                    .iter()
                    .any(|c| c.trim_start_matches('/').eq_ignore_ascii_case(cmd))
            }) {
                return None;
            }
        }

        if let Some(pattern) = &self.pattern {
            if let Some(caps) = pattern.captures(context.text) {
                let mut matches = Vec::new();
                for i in 1..caps.len() {
                    if let Some(m) = caps.get(i) {
                        matches.push(m.as_str().to_string());
                    }
                }
                let mut captures = std::collections::HashMap::new();
                for name in pattern.capture_names().flatten() {
                    if let Some(m) = caps.name(name) {
                        captures.insert(name.to_string(), m.as_str().to_string());
                    }
                }
                Some(FilterMatchResult { matches, captures })
            } else {
                None
            }
        } else {
            Some(FilterMatchResult::default())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ChatType, MessageContext, MessageFilter};
    use crate::domain::types::{ChatId, SenderId};

    #[test]
    fn combines_filter_rules_with_and_semantics() {
        let filter = MessageFilter {
            chats: Some(vec![ChatId::new(100)]),
            incoming: Some(true),
            private: Some(true),
            commands: Some(vec!["start".to_string()]),
            pattern: Some(regex::Regex::new("^/start").unwrap()),
            ..MessageFilter::default()
        };

        assert!(filter.matches(&MessageContext {
            text: "/start payload",
            chat_id: ChatId::new(100),
            sender_id: SenderId::new(42),
            incoming: true,
            chat_type: ChatType::Private,
        }));
        assert!(!filter.matches(&MessageContext {
            text: "/start payload",
            chat_id: ChatId::new(200),
            sender_id: SenderId::new(42),
            incoming: true,
            chat_type: ChatType::Private,
        }));
    }

    #[test]
    fn command_matching_ignores_bot_suffix_and_case() {
        let filter = MessageFilter {
            commands: Some(vec!["start".to_string()]),
            ..MessageFilter::default()
        };

        assert!(filter.matches(&MessageContext {
            text: "/START@my_bot argument",
            chat_id: ChatId::new(1),
            sender_id: SenderId::new(1),
            incoming: true,
            chat_type: ChatType::Group,
        }));
    }

    #[test]
    fn command_matching_with_slashes_and_mixed_case() {
        let filter = MessageFilter {
            commands: Some(vec!["/Help".to_string()]),
            ..MessageFilter::default()
        };

        assert!(filter.matches(&MessageContext {
            text: "/help",
            chat_id: ChatId::new(1),
            sender_id: SenderId::new(1),
            incoming: true,
            chat_type: ChatType::Private,
        }));
    }

    #[test]
    fn match_context_extracts_positional_and_named_groups() {
        let filter = MessageFilter {
            pattern: Some(regex::Regex::new(r"^/ban (?<user_id>\d+) (?<reason>.+)$").unwrap()),
            ..MessageFilter::default()
        };

        let result = filter
            .match_context(&MessageContext {
                text: "/ban 123456 spamming messages",
                chat_id: ChatId::new(1),
                sender_id: SenderId::new(1),
                incoming: true,
                chat_type: ChatType::Group,
            })
            .unwrap();

        assert_eq!(result.matches, vec!["123456", "spamming messages"]);
        assert_eq!(
            result.captures.get("user_id").map(|s| s.as_str()),
            Some("123456")
        );
        assert_eq!(
            result.captures.get("reason").map(|s| s.as_str()),
            Some("spamming messages")
        );
    }
}
