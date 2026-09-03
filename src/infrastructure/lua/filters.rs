//! Message filter parsing from Lua tables into domain MessageFilter models.

use crate::domain::automation::MessageFilter;
use mlua::{Table, Value};

/// Converts Lua filter values into the domain filter model.
pub fn parse_message_filter(table: &Table) -> Result<MessageFilter, mlua::Error> {
    let mut filter = MessageFilter {
        chats: parse_integer_list(table, "chats")?,
        senders: parse_integer_list(table, "senders")?,
        incoming: parse_boolean(table, "incoming")?,
        outgoing: parse_boolean(table, "outgoing")?,
        private: parse_boolean(table, "private")?,
        group: parse_boolean(table, "group")?,
        channel: parse_boolean(table, "channel")?,
        has_text: parse_boolean(table, "has_text")?,
        commands: parse_string_list(table, "commands")?.map(|commands| {
            commands
                .into_iter()
                .map(|command| command.trim_start_matches('/').to_lowercase())
                .collect()
        }),
        ..MessageFilter::default()
    };

    if let Some(pattern) = parse_string(table, "pattern")? {
        filter.pattern = Some(regex::Regex::new(&pattern).map_err(|error| {
            mlua::Error::RuntimeError(format!("invalid 'pattern' regex '{pattern}': {error}"))
        })?);
    }
    Ok(filter)
}

pub fn parse_boolean(table: &Table, key: &str) -> Result<Option<bool>, mlua::Error> {
    match table.get::<Value>(key)? {
        Value::Nil => Ok(None),
        Value::Boolean(value) => Ok(Some(value)),
        value => Err(invalid_filter_type(key, "boolean", &value)),
    }
}

pub fn parse_string(table: &Table, key: &str) -> Result<Option<String>, mlua::Error> {
    match table.get::<Value>(key)? {
        Value::Nil => Ok(None),
        Value::String(value) => Ok(Some(value.to_str()?.to_string())),
        value => Err(invalid_filter_type(key, "string", &value)),
    }
}

pub fn parse_integer_list(table: &Table, key: &str) -> Result<Option<Vec<i64>>, mlua::Error> {
    match table.get::<Value>(key)? {
        Value::Nil => Ok(None),
        Value::Integer(value) => Ok(Some(vec![value])),
        Value::Table(values) => {
            let values = values
                .sequence_values::<i64>()
                .collect::<Result<Vec<_>, _>>()?;
            if values.is_empty() {
                return Err(mlua::Error::RuntimeError(format!(
                    "filter '{key}' cannot be empty"
                )));
            }
            Ok(Some(values))
        }
        value => Err(invalid_filter_type(key, "integer or integer array", &value)),
    }
}

pub fn parse_string_list(table: &Table, key: &str) -> Result<Option<Vec<String>>, mlua::Error> {
    match table.get::<Value>(key)? {
        Value::Nil => Ok(None),
        Value::String(value) => Ok(Some(vec![value.to_str()?.to_string()])),
        Value::Table(values) => {
            let values = values
                .sequence_values::<String>()
                .collect::<Result<Vec<_>, _>>()?;
            if values.is_empty() {
                return Err(mlua::Error::RuntimeError(format!(
                    "filter '{key}' cannot be empty"
                )));
            }
            Ok(Some(values))
        }
        value => Err(invalid_filter_type(key, "string or string array", &value)),
    }
}

fn invalid_filter_type(key: &str, expected: &str, value: &Value) -> mlua::Error {
    mlua::Error::RuntimeError(format!(
        "filter '{key}' must be {expected}, got {}",
        value.type_name()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::automation::{ChatType, MessageContext};
    use mlua::Lua;

    #[test]
    fn test_parse_combined_filter() {
        let lua = Lua::new();
        let table = lua
            .load(
                r#"return {
                    chats = { 100, 200 },
                    senders = 42,
                    incoming = true,
                    private = true,
                    has_text = true,
                    commands = { "start", "/help" },
                    pattern = "^/start"
                }"#,
            )
            .eval()
            .unwrap();
        let filter = parse_message_filter(&table).unwrap();

        assert!(filter.matches(&MessageContext {
            text: "/start payload",
            chat_id: 100,
            sender_id: 42,
            incoming: true,
            chat_type: ChatType::Private,
        }));
        assert!(!filter.matches(&MessageContext {
            text: "/start payload",
            chat_id: 300,
            sender_id: 42,
            incoming: true,
            chat_type: ChatType::Private,
        }));
    }
}
