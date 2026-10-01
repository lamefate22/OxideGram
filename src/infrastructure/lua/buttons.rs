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
        _ => MessageMarkup::default(),
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
                    let data_str = std::str::from_utf8(data).unwrap_or("");
                    btn_table.set("callback_data", data_str)?;
                    btn_table.set("raw_data", lua.create_string(data)?)?;
                }
                ButtonKind::Url(url) => {
                    btn_table.set("is_callback", false)?;
                    btn_table.set("is_url", true)?;
                    btn_table.set("is_text", false)?;
                    btn_table.set("url", url.clone())?;
                }
                ButtonKind::Text => {
                    btn_table.set("is_callback", false)?;
                    btn_table.set("is_url", false)?;
                    btn_table.set("is_text", true)?;
                }
                ButtonKind::Other => {
                    btn_table.set("is_callback", false)?;
                    btn_table.set("is_url", false)?;
                    btn_table.set("is_text", false)?;
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
}
