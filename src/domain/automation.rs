//! Bot scripts and message matching rules.

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

#[derive(Debug)]
pub struct MessageContext<'a> {
    pub text: &'a str,
    pub chat_id: i64,
    pub sender_id: i64,
    pub incoming: bool,
    pub chat_type: ChatType,
}

#[derive(Debug, Clone, Default)]
pub struct MessageFilter {
    pub chats: Option<Vec<i64>>,
    pub senders: Option<Vec<i64>>,
    pub incoming: Option<bool>,
    pub outgoing: Option<bool>,
    pub private: Option<bool>,
    pub group: Option<bool>,
    pub channel: Option<bool>,
    pub has_text: Option<bool>,
    pub commands: Option<Vec<String>>,
    pub pattern: Option<Regex>,
}

impl MessageFilter {
    pub fn matches(&self, context: &MessageContext<'_>) -> bool {
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
            return false;
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
                return false;
            }
        }

        !self
            .pattern
            .as_ref()
            .is_some_and(|regex| !regex.is_match(context.text))
    }
}

#[cfg(test)]
mod tests {
    use super::{ChatType, MessageContext, MessageFilter};

    #[test]
    fn combines_filter_rules_with_and_semantics() {
        let filter = MessageFilter {
            chats: Some(vec![100]),
            incoming: Some(true),
            private: Some(true),
            commands: Some(vec!["start".to_string()]),
            pattern: Some(regex::Regex::new("^/start").unwrap()),
            ..MessageFilter::default()
        };

        assert!(filter.matches(&MessageContext {
            text: "/start payload",
            chat_id: 100,
            sender_id: 42,
            incoming: true,
            chat_type: ChatType::Private,
        }));
        assert!(!filter.matches(&MessageContext {
            text: "/start payload",
            chat_id: 200,
            sender_id: 42,
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
            chat_id: 1,
            sender_id: 1,
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
            chat_id: 1,
            sender_id: 1,
            incoming: true,
            chat_type: ChatType::Private,
        }));
    }
}
