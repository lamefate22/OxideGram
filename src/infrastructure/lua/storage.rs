//! Persistent key-value storage for Lua bots with automatic JSON serialization.

use crate::infrastructure::IO_TIMEOUT;
use mlua::{Lua, Table, Value};
use serde_json::Value as JsonValue;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, warn};

/// Persistent Bot Storage backed by a local JSON file.
#[derive(Debug, Clone)]
pub struct BotStorage {
    file_path: PathBuf,
    data: Arc<RwLock<BTreeMap<String, JsonValue>>>,
}

impl BotStorage {
    /// Creates or opens a storage instance for a specific bot script.
    pub async fn open<P: AsRef<Path>>(storage_dir: P, script_name: &str) -> Self {
        let dir = storage_dir.as_ref();
        if let Err(e) = tokio::fs::create_dir_all(dir).await {
            warn!(path = %dir.display(), error = %e, "Failed to create storage directory");
        }

        let clean_name = script_name
            .trim_end_matches(".lua")
            .replace(['/', '\\'], "_");
        let file_path = dir.join(format!("{clean_name}.json"));

        let mut initial_data = BTreeMap::new();
        if file_path.exists() {
            match tokio::time::timeout(IO_TIMEOUT, tokio::fs::read_to_string(&file_path)).await {
                Ok(Ok(content)) => {
                    match serde_json::from_str::<BTreeMap<String, JsonValue>>(&content) {
                        Ok(parsed) => {
                            debug!(
                                path = %file_path.display(),
                                entries = parsed.len(),
                                "Loaded bot storage"
                            );
                            initial_data = parsed;
                        }
                        Err(e) => {
                            error!(
                                path = %file_path.display(),
                                error = %e,
                                "Corrupted bot storage JSON, initializing empty"
                            );
                        }
                    }
                }
                Ok(Err(e)) => {
                    warn!(path = %file_path.display(), error = %e, "Could not read bot storage file");
                }
                Err(_) => {
                    warn!(path = %file_path.display(), "Reading bot storage timed out");
                }
            }
        }

        Self {
            file_path,
            data: Arc::new(RwLock::new(initial_data)),
        }
    }

    /// Persists the current in-memory state to disk as pretty JSON.
    pub async fn flush(&self) -> Result<(), String> {
        let json = {
            let guard = self.data.read().await;
            serde_json::to_string_pretty(&*guard)
                .map_err(|e| format!("JSON serialize error: {e}"))?
        };

        tokio::time::timeout(IO_TIMEOUT, tokio::fs::write(&self.file_path, json))
            .await
            .map_err(|_| "Writing bot storage timed out".to_string())?
            .map_err(|e| format!("Failed to write bot storage file: {e}"))?;

        Ok(())
    }

    /// Returns a copy of all current key-value pairs stored in memory.
    pub async fn get_all(&self) -> BTreeMap<String, JsonValue> {
        self.data.read().await.clone()
    }
}

/// Converts a `serde_json::Value` into an equivalent `mlua::Value`.
pub fn json_to_lua(lua: &Lua, json: &JsonValue) -> Result<Value, mlua::Error> {
    match json {
        JsonValue::Null => Ok(Value::Nil),
        JsonValue::Bool(b) => Ok(Value::Boolean(*b)),
        JsonValue::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(Value::Integer(i))
            } else if let Some(f) = n.as_f64() {
                Ok(Value::Number(f))
            } else {
                Ok(Value::Nil)
            }
        }
        JsonValue::String(s) => Ok(Value::String(lua.create_string(s)?)),
        JsonValue::Array(arr) => {
            let table = lua.create_table()?;
            for (idx, item) in arr.iter().enumerate() {
                table.set(idx + 1, json_to_lua(lua, item)?)?;
            }
            Ok(Value::Table(table))
        }
        JsonValue::Object(map) => {
            let table = lua.create_table()?;
            for (key, val) in map {
                table.set(key.as_str(), json_to_lua(lua, val)?)?;
            }
            Ok(Value::Table(table))
        }
    }
}

/// Converts an `mlua::Value` into an equivalent `serde_json::Value`.
pub fn lua_to_json(val: Value) -> Result<JsonValue, mlua::Error> {
    match val {
        Value::Nil => Ok(JsonValue::Null),
        Value::Boolean(b) => Ok(JsonValue::Bool(b)),
        Value::Integer(i) => Ok(JsonValue::from(i)),
        Value::Number(n) => Ok(serde_json::Number::from_f64(n)
            .map(JsonValue::Number)
            .unwrap_or(JsonValue::Null)),
        Value::String(s) => {
            let str_val = s.to_str()?;
            Ok(JsonValue::String(str_val.to_string()))
        }
        Value::Table(t) => {
            let len = t.raw_len();
            if len > 0 {
                let mut list = Vec::new();
                for i in 1..=len {
                    let item: Value = t.raw_get(i)?;
                    list.push(lua_to_json(item)?);
                }
                Ok(JsonValue::Array(list))
            } else {
                let mut map = serde_json::Map::new();
                for pair in t.pairs::<String, Value>() {
                    let (k, v) = pair?;
                    map.insert(k, lua_to_json(v)?);
                }
                Ok(JsonValue::Object(map))
            }
        }
        _ => Err(mlua::Error::RuntimeError(
            "Unsupported type for storage serialization".into(),
        )),
    }
}

/// Binds the `ox.storage` table with async get/set/delete/all/clear methods into Lua.
pub fn register_storage_api(
    lua: &Lua,
    ox_table: &Table,
    storage: BotStorage,
) -> Result<(), mlua::Error> {
    let storage_table = lua.create_table()?;

    // ox.storage.get(key, [default])
    let store_get = storage.clone();
    let get_fn =
        lua.create_async_function(move |lua, (key, default_val): (String, Option<Value>)| {
            let store = store_get.clone();
            async move {
                let guard = store.data.read().await;
                if let Some(val) = guard.get(&key) {
                    json_to_lua(&lua, val)
                } else {
                    Ok(default_val.unwrap_or(Value::Nil))
                }
            }
        })?;
    storage_table.set("get", get_fn)?;

    // ox.storage.set(key, value)
    let store_set = storage.clone();
    let set_fn = lua.create_async_function(move |_, (key, val): (String, Value)| {
        let store = store_set.clone();
        async move {
            let json_val = lua_to_json(val)?;
            {
                let mut guard = store.data.write().await;
                guard.insert(key, json_val);
            }
            if let Err(e) = store.flush().await {
                warn!(error = %e, "Failed to flush bot storage to disk");
            }
            Ok(())
        }
    })?;
    storage_table.set("set", set_fn)?;

    // ox.storage.has(key) -> boolean
    let store_has = storage.clone();
    let has_fn = lua.create_async_function(move |_, key: String| {
        let store = store_has.clone();
        async move {
            let guard = store.data.read().await;
            Ok(guard.contains_key(&key))
        }
    })?;
    storage_table.set("has", has_fn)?;

    // ox.storage.delete(key)
    let store_del = storage.clone();
    let del_fn = lua.create_async_function(move |_, key: String| {
        let store = store_del.clone();
        async move {
            let removed = {
                let mut guard = store.data.write().await;
                guard.remove(&key).is_some()
            };
            if removed {
                let _ = store.flush().await;
            }
            Ok(removed)
        }
    })?;
    storage_table.set("delete", del_fn.clone())?;
    storage_table.set("remove", del_fn)?;

    // ox.storage.all() -> table
    let store_all = storage.clone();
    let all_fn = lua.create_async_function(move |lua, ()| {
        let store = store_all.clone();
        async move {
            let guard = store.data.read().await;
            let table = lua.create_table()?;
            for (k, v) in guard.iter() {
                table.set(k.as_str(), json_to_lua(&lua, v)?)?;
            }
            Ok(table)
        }
    })?;
    storage_table.set("all", all_fn)?;

    // ox.storage.clear()
    let store_clear = storage;
    let clear_fn = lua.create_async_function(move |_, ()| {
        let store = store_clear.clone();
        async move {
            {
                let mut guard = store.data.write().await;
                guard.clear();
            }
            let _ = store.flush().await;
            Ok(())
        }
    })?;
    storage_table.set("clear", clear_fn)?;

    ox_table.set("storage", storage_table)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_storage_roundtrip_and_serialization() {
        let temp_dir = std::env::temp_dir().join(format!("oxide_test_{}", rand::random::<u32>()));
        let storage = BotStorage::open(&temp_dir, "test_bot").await;

        let lua = Lua::new();
        let ox = lua.create_table().unwrap();
        register_storage_api(&lua, &ox, storage.clone()).unwrap();
        lua.globals().set("ox", ox).unwrap();

        // Set value in Lua
        lua.load(r#"ox.storage.set("counter", 10)"#)
            .exec_async()
            .await
            .unwrap();
        lua.load(r#"ox.storage.set("user", { name = "Alice", active = true })"#)
            .exec_async()
            .await
            .unwrap();

        // Get value in Lua
        let counter: i64 = lua
            .load(r#"return ox.storage.get("counter")"#)
            .eval_async()
            .await
            .unwrap();
        assert_eq!(counter, 10);

        let default_val: String = lua
            .load(r#"return ox.storage.get("unknown_key", "fallback")"#)
            .eval_async()
            .await
            .unwrap();
        assert_eq!(default_val, "fallback");

        let active: bool = lua
            .load(r#"return ox.storage.get("user").active"#)
            .eval_async()
            .await
            .unwrap();
        assert!(active);

        // Verify disk file exists and can be reloaded
        let storage2 = BotStorage::open(&temp_dir, "test_bot").await;
        let guard = storage2.data.read().await;
        assert_eq!(guard.get("counter"), Some(&JsonValue::from(10)));

        let _ = tokio::fs::remove_dir_all(&temp_dir).await;
    }
}
