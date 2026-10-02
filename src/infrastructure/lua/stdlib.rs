//! Standard library extensions for Lua: string methods, global helpers, and randomization.

use mlua::{Lua, Table, Value};

/// Escapes characters for Telegram MarkdownV2 formatting.
pub fn escape_markdown_v2(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 16);
    for ch in text.chars() {
        if matches!(
            ch,
            '_' | '*'
                | '['
                | ']'
                | '('
                | ')'
                | '~'
                | '`'
                | '>'
                | '#'
                | '+'
                | '-'
                | '='
                | '|'
                | '{'
                | '}'
                | '.'
                | '!'
                | '\\'
        ) {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

/// Escapes characters for Telegram HTML formatting.
pub fn escape_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 16);
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            other => out.push(other),
        }
    }
    out
}

/// Extracts a slash command name and argument string from input text.
///
/// Returns `(Some(cmd_name_lower), arguments_string)` if text starts with `/`,
/// stripping any bot mention suffix (e.g. `/start@mybot`).
/// Returns `(None, original_text)` otherwise.
pub fn extract_command(text: &str) -> (Option<String>, String) {
    let trimmed = text.trim();
    if !trimmed.starts_with('/') {
        return (None, trimmed.to_string());
    }

    let without_slash = &trimmed[1..];
    let (cmd_part, args_part) = match without_slash.split_once(char::is_whitespace) {
        Some((cmd, rest)) => (cmd, rest.trim()),
        None => (without_slash, ""),
    };

    let cmd_name = match cmd_part.split_once('@') {
        Some((cmd, _)) => cmd,
        None => cmd_part,
    };

    (Some(cmd_name.to_lowercase()), args_part.to_string())
}

/// Registers string metatable extensions and global string helpers in Lua.
pub fn register_stdlib(lua: &Lua, ox_table: Option<&Table>) -> Result<(), mlua::Error> {
    // 1. Extend string metatable so that every string gets :contains, :starts_with, etc.
    let string_meta: Table = lua.load("return getmetatable('')").eval()?;
    let string_index: Table = string_meta.get("__index")?;

    // contains: str:contains(sub, [case_insensitive])
    let contains_fn = lua.create_function(|_, (s, sub, ci): (String, String, Option<bool>)| {
        let case_insensitive = ci.unwrap_or(true);
        if case_insensitive {
            Ok(s.to_lowercase().contains(&sub.to_lowercase()))
        } else {
            Ok(s.contains(&sub))
        }
    })?;
    string_index.set("contains", contains_fn.clone())?;

    // starts_with: str:starts_with(prefix, [case_insensitive])
    let starts_with_fn =
        lua.create_function(|_, (s, prefix, ci): (String, String, Option<bool>)| {
            let case_insensitive = ci.unwrap_or(true);
            if case_insensitive {
                Ok(s.to_lowercase().starts_with(&prefix.to_lowercase()))
            } else {
                Ok(s.starts_with(&prefix))
            }
        })?;
    string_index.set("starts_with", starts_with_fn.clone())?;

    // ends_with: str:ends_with(suffix, [case_insensitive])
    let ends_with_fn =
        lua.create_function(|_, (s, suffix, ci): (String, String, Option<bool>)| {
            let case_insensitive = ci.unwrap_or(true);
            if case_insensitive {
                Ok(s.to_lowercase().ends_with(&suffix.to_lowercase()))
            } else {
                Ok(s.ends_with(&suffix))
            }
        })?;
    string_index.set("ends_with", ends_with_fn.clone())?;

    // split: str:split(delimiter) -> array table
    let split_fn = lua.create_function(|lua, (s, delimiter): (String, Option<String>)| {
        let delim = delimiter.unwrap_or_else(|| " ".to_string());
        let table = lua.create_table()?;
        if delim.is_empty() {
            for (idx, ch) in s.chars().enumerate() {
                table.set(idx + 1, ch.to_string())?;
            }
        } else {
            for (idx, part) in s.split(&delim).enumerate() {
                table.set(idx + 1, part.to_string())?;
            }
        }
        Ok(table)
    })?;
    string_index.set("split", split_fn.clone())?;

    // trim: str:trim() -> trimmed string
    let trim_fn = lua.create_function(|_, s: String| Ok(s.trim().to_string()))?;
    string_index.set("trim", trim_fn.clone())?;

    // to_lower / to_upper: Unicode-aware casing
    let to_lower_fn = lua.create_function(|_, s: String| Ok(s.to_lowercase()))?;
    string_index.set("to_lower", to_lower_fn.clone())?;

    let to_upper_fn = lua.create_function(|_, s: String| Ok(s.to_uppercase()))?;
    string_index.set("to_upper", to_upper_fn.clone())?;

    // replace: str:replace(from, to, [is_regex])
    let replace_fn = lua.create_function(
        |_, (s, from_pat, to, is_regex): (String, String, String, Option<bool>)| {
            if is_regex.unwrap_or(false) {
                let re = regex::Regex::new(&from_pat)
                    .map_err(|e| mlua::Error::RuntimeError(format!("Invalid regex: {e}")))?;
                Ok(re.replace_all(&s, to.as_str()).to_string())
            } else {
                Ok(s.replace(&from_pat, &to))
            }
        },
    )?;
    string_index.set("replace", replace_fn.clone())?;

    // strip_prefix: str:strip_prefix(prefix)
    let strip_prefix_fn = lua.create_function(|_, (s, prefix): (String, String)| {
        Ok(s.strip_prefix(&prefix).unwrap_or(&s).to_string())
    })?;
    string_index.set("strip_prefix", strip_prefix_fn.clone())?;

    // strip_suffix: str:strip_suffix(suffix)
    let strip_suffix_fn = lua.create_function(|_, (s, suffix): (String, String)| {
        Ok(s.strip_suffix(&suffix).unwrap_or(&s).to_string())
    })?;
    string_index.set("strip_suffix", strip_suffix_fn.clone())?;

    // pad_left: str:pad_left(target_len, [pad_char])
    let pad_left_fn =
        lua.create_function(|_, (s, len, pad_char): (String, usize, Option<String>)| {
            let char_count = s.chars().count();
            if char_count >= len {
                return Ok(s);
            }
            let pad = pad_char.as_deref().unwrap_or(" ");
            let needed = len - char_count;
            let pad_str = pad.repeat(needed);
            Ok(format!("{pad_str}{s}"))
        })?;
    string_index.set("pad_left", pad_left_fn.clone())?;

    // pad_right: str:pad_right(target_len, [pad_char])
    let pad_right_fn =
        lua.create_function(|_, (s, len, pad_char): (String, usize, Option<String>)| {
            let char_count = s.chars().count();
            if char_count >= len {
                return Ok(s);
            }
            let pad = pad_char.as_deref().unwrap_or(" ");
            let needed = len - char_count;
            let pad_str = pad.repeat(needed);
            Ok(format!("{s}{pad_str}"))
        })?;
    string_index.set("pad_right", pad_right_fn.clone())?;

    // is_empty: str:is_empty()
    let is_empty_fn = lua.create_function(|_, s: String| Ok(s.is_empty()))?;
    string_index.set("is_empty", is_empty_fn.clone())?;

    // is_blank: str:is_blank()
    let is_blank_fn = lua.create_function(|_, s: String| Ok(s.trim().is_empty()))?;
    string_index.set("is_blank", is_blank_fn.clone())?;

    // escape_markdown: str:escape_markdown()
    let escape_markdown_fn = lua.create_function(|_, s: String| Ok(escape_markdown_v2(&s)))?;
    string_index.set("escape_markdown", escape_markdown_fn.clone())?;

    // escape_html: str:escape_html()
    let escape_html_fn = lua.create_function(|_, s: String| Ok(escape_html(&s)))?;
    string_index.set("escape_html", escape_html_fn.clone())?;

    // extract_command: str:extract_command() -> (cmd, args)
    let extract_command_fn = lua.create_function(|lua, s: String| {
        let (cmd, args) = extract_command(&s);
        let cmd_val = match cmd {
            Some(c) => Value::String(lua.create_string(&c)?),
            None => Value::Nil,
        };
        let args_val = Value::String(lua.create_string(&args)?);
        Ok((cmd_val, args_val))
    })?;
    string_index.set("extract_command", extract_command_fn.clone())?;

    // lines: str:lines() -> array table
    let lines_fn = lua.create_function(|lua, s: String| {
        let tbl = lua.create_table()?;
        for (idx, line) in s.lines().enumerate() {
            tbl.set(idx + 1, line.to_string())?;
        }
        Ok(tbl)
    })?;
    string_index.set("lines", lines_fn.clone())?;

    // 2. Register ox.string table and direct shortcuts on ox if ox_table provided
    if let Some(ox) = ox_table {
        let ox_string = lua.create_table()?;
        ox_string.set("contains", contains_fn)?;
        ox_string.set("starts_with", starts_with_fn)?;
        ox_string.set("ends_with", ends_with_fn)?;
        ox_string.set("split", split_fn)?;
        ox_string.set("trim", trim_fn)?;
        ox_string.set("to_lower", to_lower_fn)?;
        ox_string.set("to_upper", to_upper_fn)?;
        ox_string.set("replace", replace_fn)?;
        ox_string.set("strip_prefix", strip_prefix_fn)?;
        ox_string.set("strip_suffix", strip_suffix_fn)?;
        ox_string.set("pad_left", pad_left_fn)?;
        ox_string.set("pad_right", pad_right_fn)?;
        ox_string.set("is_empty", is_empty_fn)?;
        ox_string.set("is_blank", is_blank_fn)?;
        ox_string.set("escape_markdown", escape_markdown_fn.clone())?;
        ox_string.set("escape_html", escape_html_fn.clone())?;
        ox_string.set("extract_command", extract_command_fn)?;
        ox_string.set("lines", lines_fn)?;

        ox.set("string", ox_string)?;
        ox.set("escape_markdown", escape_markdown_fn)?;
        ox.set("escape_html", escape_html_fn)?;
    }

    Ok(())
}

/// Registers randomization and timing utility methods onto `ox` table.
pub fn register_random_helpers(lua: &Lua, ox_table: &Table) -> Result<(), mlua::Error> {
    // ox.sleep_random(min_secs, max_secs)
    let sleep_random_fn =
        lua.create_async_function(|_, (min_secs, max_secs): (f64, f64)| async move {
            let (low, high) = if min_secs <= max_secs {
                (min_secs, max_secs)
            } else {
                (max_secs, min_secs)
            };
            let duration = low + rand::random::<f64>() * (high - low);
            tokio::time::sleep(std::time::Duration::from_secs_f64(duration.max(0.0))).await;
            Ok(duration)
        })?;
    ox_table.set("sleep_random", sleep_random_fn)?;

    // ox.choice(table) -> random element
    let choice_fn = lua.create_function(|_, table: Table| {
        let len = table.raw_len();
        if len == 0 {
            return Ok(Value::Nil);
        }
        let rand_idx = (rand::random::<f64>() * len as f64).floor() as usize + 1;
        table.raw_get(rand_idx)
    })?;
    ox_table.set("choice", choice_fn)?;

    // ox.random_int(min, max) -> random integer
    let random_int_fn = lua.create_function(|_, (min, max): (i64, i64)| {
        let (low, high) = if min <= max { (min, max) } else { (max, min) };
        let diff = (high - low + 1) as f64;
        let rand_val = low + (rand::random::<f64>() * diff).floor() as i64;
        Ok(rand_val)
    })?;
    ox_table.set("random_int", random_int_fn)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_string_metatable_extensions_and_globals() {
        let lua = Lua::new();
        let ox = lua.create_table().unwrap();
        register_stdlib(&lua, Some(&ox)).unwrap();

        // Method call on string
        let res1: bool = lua
            .load(r#"("💋 Привет, мир! 💌"):contains("привет")"#)
            .eval()
            .unwrap();
        assert!(res1);

        let res2: bool = lua
            .load(r#"("💋 Привет"):starts_with("💋")"#)
            .eval()
            .unwrap();
        assert!(res2);

        let res3: bool = lua
            .load(r#"("Тест.txt"):ends_with(".TXT")"#)
            .eval()
            .unwrap();
        assert!(res3);

        let res4: String = lua.load(r#"("   пробелы   "):trim()"#).eval().unwrap();
        assert_eq!(res4, "пробелы");

        // Split
        let parts: Vec<String> = lua
            .load(r#"("один,два,три"):split(",")"#)
            .eval::<Table>()
            .unwrap()
            .sequence_values::<String>()
            .map(|r| r.unwrap())
            .collect();
        assert_eq!(parts, vec!["один", "два", "три"]);

        // Module functions via ox.string
        lua.globals().set("ox", ox).unwrap();
        let res_mod: bool = lua
            .load(r#"ox.string.contains("текст", "ЕКС")"#)
            .eval()
            .unwrap();
        assert!(res_mod);

        // Ensure global namespace is clean
        let has_global: bool = lua.load(r#"return contains ~= nil"#).eval().unwrap();
        assert!(!has_global);
    }

    #[test]
    fn test_new_string_helpers() {
        let lua = Lua::new();
        let ox = lua.create_table().unwrap();
        register_stdlib(&lua, Some(&ox)).unwrap();
        lua.globals().set("ox", ox).unwrap();

        // replace
        let r1: String = lua
            .load(r#"("hello world"):replace("world", "Rust")"#)
            .eval()
            .unwrap();
        assert_eq!(r1, "hello Rust");

        // regex replacement with \\d+
        let r2_rust: String = lua
            .load(r#"("abc 123 def"):replace("\\d+", "NUM", true)"#)
            .eval()
            .unwrap();
        assert_eq!(r2_rust, "abc NUM def");

        // strip_prefix & strip_suffix
        let s1: String = lua.load(r#"("/start"):strip_prefix("/")"#).eval().unwrap();
        assert_eq!(s1, "start");

        let s2: String = lua
            .load(r#"("archive.tar.gz"):strip_suffix(".gz")"#)
            .eval()
            .unwrap();
        assert_eq!(s2, "archive.tar");

        // pad_left & pad_right
        let p1: String = lua.load(r#"("42"):pad_left(5, "0")"#).eval().unwrap();
        assert_eq!(p1, "00042");

        let p2: String = lua.load(r#"("item"):pad_right(8, ".")"#).eval().unwrap();
        assert_eq!(p2, "item....");

        // is_empty & is_blank
        assert!(lua.load(r#"(""):is_empty()"#).eval::<bool>().unwrap());
        assert!(!lua.load(r#"("   "):is_empty()"#).eval::<bool>().unwrap());
        assert!(lua.load(r#"("   "):is_blank()"#).eval::<bool>().unwrap());
        assert!(!lua.load(r#"("  a "):is_blank()"#).eval::<bool>().unwrap());

        // escape_markdown & escape_html
        let md: String = lua
            .load(r#"ox.escape_markdown("Hello [World]! *bold*")"#)
            .eval()
            .unwrap();
        assert_eq!(md, r#"Hello \[World\]\! \*bold\*"#);

        let html: String = lua
            .load(r#"ox.escape_html("<hello & 'world'>")"#)
            .eval()
            .unwrap();
        assert_eq!(html, "&lt;hello &amp; &#39;world&#39;&gt;");

        // extract_command via string method and ox.string
        let (cmd, args): (Option<String>, String) = lua
            .load(r#"return ("/ban@mybot 12345 7d spam"):extract_command()"#)
            .eval()
            .unwrap();
        assert_eq!(cmd, Some("ban".into()));
        assert_eq!(args, "12345 7d spam");

        let (cmd2, args2): (Option<String>, String) = lua
            .load(r#"return ox.string.extract_command("/kick 999")"#)
            .eval()
            .unwrap();
        assert_eq!(cmd2, Some("kick".into()));
        assert_eq!(args2, "999");

        // lines
        let lines: Vec<String> = lua
            .load("return ('line1\\nline2\\nline3'):lines()")
            .eval::<Table>()
            .unwrap()
            .sequence_values::<String>()
            .map(|r| r.unwrap())
            .collect();
        assert_eq!(lines, vec!["line1", "line2", "line3"]);
    }
}
