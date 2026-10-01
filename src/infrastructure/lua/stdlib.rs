//! Standard library extensions for Lua: string methods, global helpers, and randomization.

use mlua::{Lua, Table, Value};

/// Registers string metatable extensions and global string helpers in Lua.
pub fn register_stdlib(lua: &Lua) -> Result<(), mlua::Error> {
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
    string_index.set("to_lower", to_lower_fn)?;

    let to_upper_fn = lua.create_function(|_, s: String| Ok(s.to_uppercase()))?;
    string_index.set("to_upper", to_upper_fn)?;

    // 2. Global helper functions (for backwards compatibility and direct calls)
    let globals = lua.globals();
    globals.set("contains", contains_fn)?;
    globals.set("starts_with", starts_with_fn)?;
    globals.set("ends_with", ends_with_fn)?;
    globals.set("split", split_fn)?;
    globals.set("trim", trim_fn)?;

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
        register_stdlib(&lua).unwrap();

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

        // Global functions
        let res_glob: bool = lua.load(r#"contains("текст", "ЕКС")"#).eval().unwrap();
        assert!(res_glob);
    }
}
