//! Parser and converters for Telegram message keyboards and button click handlers.

use crate::domain::automation::{BotButton, ButtonKind, KeyboardRow, MessageMarkup};
use grammers_tl_types as tl;
use mlua::Lua;

/// Parses Telegram MTProto `ReplyMarkup` enum into domain `MessageMarkup`.
pub fn parse_reply_markup(markup: Option<&tl::enums::ReplyMarkup>) -> MessageMarkup {
    let Some(markup) = markup else {
        return MessageMarkup::default();
    };

    match markup {
        tl::enums::ReplyMarkup::ReplyInlineMarkup(m) => {
            let mut rows = Vec::new();
            for tl::enums::KeyboardInlineButtonRow::Row(row) in &m.rows {
                let mut buttons = Vec::new();
                for tl::enums::KeyboardInlineButton::Button(btn) in &row.buttons {
                    let kind = match &btn.r#type {
                        tl::enums::InlineButtonType::Callback(cb) => {
                            ButtonKind::Callback(cb.data.clone())
                        }
                        tl::enums::InlineButtonType::Url(u) => ButtonKind::Url(u.url.clone()),
                        _ => ButtonKind::Other,
                    };
                    buttons.push(BotButton::new(&btn.text, kind));
                }
                rows.push(KeyboardRow { buttons });
            }
            MessageMarkup {
                rows,
                is_inline: true,
            }
        }
        tl::enums::ReplyMarkup::ReplyKeyboardMarkup(m) => {
            let mut rows = Vec::new();
            for tl::enums::KeyboardButtonRow::Row(row) in &m.rows {
                let mut buttons = Vec::new();
                for tl::enums::KeyboardButton::Button(btn) in &row.buttons {
                    buttons.push(BotButton::new(&btn.text, ButtonKind::Text));
                }
                rows.push(KeyboardRow { buttons });
            }
            MessageMarkup {
                rows,
                is_inline: false,
            }
        }
        tl::enums::ReplyMarkup::ReplyKeyboardHide(_) => MessageMarkup::default(),
        _ => MessageMarkup::default(),
    }
}

/// Action to perform on chat active keyboard based on received `ReplyMarkup`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplyMarkupAction {
    /// Message contains an inline keyboard.
    Inline(MessageMarkup),
    /// Message sets or updates the persistent reply keyboard for the chat.
    ReplyKeyboard(MessageMarkup),
    /// Message explicitly removes/hides the reply keyboard for the chat.
    HideKeyboard,
    /// Message has no keyboard markup.
    None,
}

/// Parses Telegram MTProto `ReplyMarkup` into a `ReplyMarkupAction` to maintain chat keyboard state.
pub fn parse_reply_markup_action(markup: Option<&tl::enums::ReplyMarkup>) -> ReplyMarkupAction {
    let Some(markup) = markup else {
        return ReplyMarkupAction::None;
    };

    match markup {
        tl::enums::ReplyMarkup::ReplyInlineMarkup(_) => {
            ReplyMarkupAction::Inline(parse_reply_markup(Some(markup)))
        }
        tl::enums::ReplyMarkup::ReplyKeyboardMarkup(_) => {
            ReplyMarkupAction::ReplyKeyboard(parse_reply_markup(Some(markup)))
        }
        tl::enums::ReplyMarkup::ReplyKeyboardHide(_) => ReplyMarkupAction::HideKeyboard,
        _ => ReplyMarkupAction::None,
    }
}

/// Converts `MessageMarkup` into a Lua array of rows, where each row is an array of button tables.
pub fn markup_to_lua_table(lua: &Lua, markup: &MessageMarkup) -> mlua::Result<mlua::Table> {
    let root = lua.create_table()?;
    for (r_idx, row) in markup.rows.iter().enumerate() {
        let row_table = lua.create_table()?;
        for (c_idx, btn) in row.buttons.iter().enumerate() {
            let btn_table = lua.create_table()?;
            btn_table.set("text", btn.text.clone())?;
            btn_table.set("row", r_idx + 1)?;
            btn_table.set("col", c_idx + 1)?;
            match &btn.kind {
                ButtonKind::Callback(data) => {
                    btn_table.set("is_callback", true)?;
                    btn_table.set("is_url", false)?;
                    btn_table.set("is_text", false)?;
                    btn_table.set("type", "callback")?;
                    let data_str = std::str::from_utf8(data).unwrap_or("");
                    btn_table.set("callback_data", data_str)?;
                    btn_table.set("data", data_str)?;
                    btn_table.set("raw_data", lua.create_string(data)?)?;
                }
                ButtonKind::Url(url) => {
                    btn_table.set("is_callback", false)?;
                    btn_table.set("is_url", true)?;
                    btn_table.set("is_text", false)?;
                    btn_table.set("type", "url")?;
                    btn_table.set("url", url.clone())?;
                }
                ButtonKind::Text => {
                    btn_table.set("is_callback", false)?;
                    btn_table.set("is_url", false)?;
                    btn_table.set("is_text", true)?;
                    btn_table.set("type", "text")?;
                }
                ButtonKind::Other => {
                    btn_table.set("is_callback", false)?;
                    btn_table.set("is_url", false)?;
                    btn_table.set("is_text", false)?;
                    btn_table.set("type", "other")?;
                }
            }
            row_table.set(c_idx + 1, btn_table)?;
        }
        root.set(r_idx + 1, row_table)?;
    }
    Ok(root)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_inline_callback_and_url_buttons() {
        let markup = tl::enums::ReplyMarkup::ReplyInlineMarkup(tl::types::ReplyInlineMarkup {
            force_reply: false,
            rows: vec![tl::enums::KeyboardInlineButtonRow::Row(
                tl::types::KeyboardInlineButtonRow {
                    buttons: vec![
                        tl::enums::KeyboardInlineButton::Button(tl::types::KeyboardInlineButton {
                            style: None,
                            text: "Click Me".into(),
                            r#type: tl::enums::InlineButtonType::Callback(
                                tl::types::InlineButtonTypeCallback {
                                    requires_password: false,
                                    data: b"btn_action_1".to_vec(),
                                },
                            ),
                        }),
                        tl::enums::KeyboardInlineButton::Button(tl::types::KeyboardInlineButton {
                            style: None,
                            text: "Open Link".into(),
                            r#type: tl::enums::InlineButtonType::Url(
                                tl::types::InlineButtonTypeUrl {
                                    url: "https://example.com".into(),
                                },
                            ),
                        }),
                    ],
                },
            )],
        });

        let parsed = parse_reply_markup(Some(&markup));
        assert!(parsed.is_inline);
        assert_eq!(parsed.rows.len(), 1);
        assert_eq!(parsed.rows[0].buttons.len(), 2);

        let (r, c, btn) = parsed.find_button("Click Me").unwrap();
        assert_eq!((r, c), (0, 0));
        assert!(btn.is_callback());
        assert_eq!(btn.callback_data(), Some(b"btn_action_1".as_slice()));

        let by_data = parsed.find_button("btn_action_1").unwrap();
        assert_eq!(by_data.2.text, "Click Me");
    }

    #[test]
    fn parses_regular_reply_keyboard_buttons() {
        let markup = tl::enums::ReplyMarkup::ReplyKeyboardMarkup(tl::types::ReplyKeyboardMarkup {
            resize: false,
            single_use: false,
            selective: false,
            persistent: false,
            force_reply: false,
            placeholder: None,
            rows: vec![tl::enums::KeyboardButtonRow::Row(
                tl::types::KeyboardButtonRow {
                    buttons: vec![tl::enums::KeyboardButton::Button(
                        tl::types::KeyboardButton {
                            style: None,
                            text: "Start Search".into(),
                            r#type: tl::enums::ButtonType::Default,
                        },
                    )],
                },
            )],
        });

        let parsed = parse_reply_markup(Some(&markup));
        assert!(!parsed.is_inline);
        assert_eq!(parsed.rows.len(), 1);
        let btn = parsed.button_at(0, 0).unwrap();
        assert_eq!(btn.text, "Start Search");
        assert_eq!(btn.kind, ButtonKind::Text);
    }

    #[test]
    fn markup_to_lua_table_produces_expected_fields() {
        let lua = Lua::new();
        let markup = MessageMarkup {
            is_inline: true,
            rows: vec![KeyboardRow {
                buttons: vec![
                    BotButton {
                        text: "Callback Btn".into(),
                        kind: ButtonKind::Callback(b"cb:123".to_vec()),
                    },
                    BotButton {
                        text: "Link Btn".into(),
                        kind: ButtonKind::Url("https://example.com".into()),
                    },
                ],
            }],
        };

        let root = markup_to_lua_table(&lua, &markup).unwrap();
        let row1: mlua::Table = root.get(1).unwrap();
        let btn1: mlua::Table = row1.get(1).unwrap();
        assert_eq!(btn1.get::<String>("text").unwrap(), "Callback Btn");
        assert_eq!(btn1.get::<i64>("row").unwrap(), 1);
        assert_eq!(btn1.get::<i64>("col").unwrap(), 1);
        assert!(btn1.get::<bool>("is_callback").unwrap());
        assert!(!btn1.get::<bool>("is_url").unwrap());
        assert!(!btn1.get::<bool>("is_text").unwrap());
        assert_eq!(btn1.get::<String>("type").unwrap(), "callback");
        assert_eq!(btn1.get::<String>("callback_data").unwrap(), "cb:123");
        assert_eq!(btn1.get::<String>("data").unwrap(), "cb:123");

        let btn2: mlua::Table = row1.get(2).unwrap();
        assert_eq!(btn2.get::<String>("text").unwrap(), "Link Btn");
        assert_eq!(btn2.get::<String>("type").unwrap(), "url");
        assert!(btn2.get::<bool>("is_url").unwrap());
        assert_eq!(btn2.get::<String>("url").unwrap(), "https://example.com");
    }
}
