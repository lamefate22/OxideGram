//! Dialog Flow Engine and Finite State Machine (FSM) for step-by-step Lua bot dialogs.

use mlua::{Function, Lua, Table, Value};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::Instant;

/// Helper to extract string from a Lua value (either String or Integer/Number).
fn value_to_string(val: Value) -> Result<String, mlua::Error> {
    match val {
        Value::String(s) => Ok(s.to_str()?.to_string()),
        Value::Integer(i) => Ok(i.to_string()),
        Value::Number(n) => Ok(n.to_string()),
        _ => Err(mlua::Error::RuntimeError(
            "expected string or number".to_string(),
        )),
    }
}

/// Helper to extract optional chat ID from Lua value.
fn value_to_chat_id(val: Option<Value>) -> Option<i64> {
    match val {
        Some(Value::Integer(i)) => Some(i),
        Some(Value::Number(n)) => Some(n as i64),
        _ => None,
    }
}

/// Configuration and state representation for a single flow step.
struct FlowStep {
    matchers: Vec<String>,
    action: Function,
    next: Option<String>,
    on_timeout: Option<Function>,
    timeout_secs: Option<f64>,
}

/// Global pattern/substring matcher triggering across any step in the flow.
struct FlowGlobalMatcher {
    pattern: String,
    action: Function,
}

/// Slash command route triggering across any step in the flow.
struct FlowCommandMatcher {
    commands: Vec<String>,
    action: Function,
}

/// State machine coordinator for a dialog flow.
struct FlowCoordinator {
    name: String,
    target_chats: Option<Vec<i64>>,
    initial_step: RwLock<Option<String>>,
    current_steps: RwLock<HashMap<i64, String>>,
    step_times: RwLock<HashMap<i64, Instant>>,
    default_timeout: Option<f64>,
    steps: RwLock<HashMap<String, FlowStep>>,
    globals: RwLock<Vec<FlowGlobalMatcher>>,
    commands: RwLock<Vec<FlowCommandMatcher>>,
}

/// Registers the `ox.flow(name, [options])` constructor in Lua.
pub fn register_flow_api(lua: &Lua, ox_table: &Table) -> Result<(), mlua::Error> {
    let flow_constructor = lua.create_function(
        |lua_outer, (flow_name, options_val): (String, Option<Table>)| {
            let mut target_chats: Option<Vec<i64>> = None;
            let mut default_timeout: Option<f64> = None;
            let mut initial_step: Option<String> = None;

            if let Some(ref opts) = options_val {
                if let Ok(chat_id) = opts
                    .get::<i64>("chats")
                    .or_else(|_| opts.get::<i64>("chat"))
                    .or_else(|_| opts.get::<i64>("target_chat"))
                {
                    target_chats = Some(vec![chat_id]);
                } else if let Ok(chats_table) = opts
                    .get::<Table>("chats")
                    .or_else(|_| opts.get::<Table>("target_chats"))
                {
                    let mut list = Vec::new();
                    for c in chats_table.sequence_values::<i64>().flatten() {
                        list.push(c);
                    }
                    target_chats = Some(list);
                }
                default_timeout = opts.get::<Option<f64>>("timeout")?;
                initial_step = opts.get::<Option<String>>("initial")?;
            }

            let coordinator = Arc::new(FlowCoordinator {
                name: flow_name.clone(),
                target_chats,
                initial_step: RwLock::new(initial_step),
                current_steps: RwLock::new(HashMap::new()),
                step_times: RwLock::new(HashMap::new()),
                default_timeout,
                steps: RwLock::new(HashMap::new()),
                globals: RwLock::new(Vec::new()),
                commands: RwLock::new(Vec::new()),
            });

            let flow_table = lua_outer.create_table()?;
            flow_table.set("name", flow_name.clone())?;

            // Shared context data table for this flow
            let ctx_data = lua_outer.create_table()?;
            flow_table.set("data", ctx_data.clone())?;

            // flow:step(name, config) OR flow.step(name, config)
            let coord_step = Arc::clone(&coordinator);
            let step_fn = lua_outer.create_function(
                move |_, (arg1, arg2, arg3): (Value, Option<Value>, Option<Value>)| {
                    let (step_name, config) = match (arg1, arg2, arg3) {
                        (Value::Table(_), Some(name_val), Some(Value::Table(config))) => {
                            (value_to_string(name_val)?, config)
                        }
                        (name_val, Some(Value::Table(config)), _) => {
                            (value_to_string(name_val)?, config)
                        }
                        _ => {
                            return Err(mlua::Error::RuntimeError(
                                "flow:step requires step_name (string) and config (table)".into(),
                            ));
                        }
                    };

                    let action: Function = config.get("action").map_err(|_| {
                        mlua::Error::RuntimeError(format!(
                            "flow step '{step_name}' missing required 'action' function"
                        ))
                    })?;
                    let next: Option<String> = config.get("next")?;
                    let on_timeout: Option<Function> = config.get("on_timeout")?;
                    let timeout_secs: Option<f64> = config.get("timeout")?;

                    let mut matchers = Vec::new();
                    if let Ok(s) = config.get::<String>("match") {
                        matchers.push(s);
                    } else if let Ok(t) = config.get::<Table>("match") {
                        for m in t.sequence_values::<String>().flatten() {
                            matchers.push(m);
                        }
                    }
                    if let Ok(s) = config.get::<String>("commands") {
                        let clean = s.trim_start_matches('/').to_string();
                        matchers.push(clean.clone());
                        matchers.push(format!("/{clean}"));
                    } else if let Ok(t) = config.get::<Table>("commands") {
                        for c in t.sequence_values::<String>().flatten() {
                            let clean = c.trim_start_matches('/').to_string();
                            matchers.push(clean.clone());
                            matchers.push(format!("/{clean}"));
                        }
                    }
                    if let Ok(p) = config.get::<String>("pattern") {
                        matchers.push(p);
                    }

                    if let Ok(mut initial) = coord_step.initial_step.write()
                        && initial.is_none()
                    {
                        *initial = Some(step_name.clone());
                    }
                    if let Ok(mut map) = coord_step.steps.write() {
                        map.insert(
                            step_name,
                            FlowStep {
                                matchers,
                                action,
                                next,
                                on_timeout,
                                timeout_secs,
                            },
                        );
                    }

                    Ok(())
                },
            )?;
            flow_table.set("step", step_fn)?;

            // flow:command(name_or_list, action) OR flow.command(name_or_list, action)
            let coord_cmd = Arc::clone(&coordinator);
            let cmd_fn = lua_outer.create_function(
                move |_, (arg1, arg2, arg3): (Value, Option<Value>, Option<Value>)| {
                    let (cmd_val, action_val) = match (arg1, arg2, arg3) {
                        (Value::Table(_), Some(n), Some(a)) => (n, a),
                        (n, Some(a), _) => (n, a),
                        _ => {
                            return Err(mlua::Error::RuntimeError(
                                "flow:command requires command name and action function".into(),
                            ));
                        }
                    };

                    let action: Function = match action_val {
                        Value::Function(f) => f,
                        _ => {
                            return Err(mlua::Error::RuntimeError(
                                "flow:command action must be a function".into(),
                            ));
                        }
                    };

                    let mut cmd_list = Vec::new();
                    match cmd_val {
                        Value::String(s) => {
                            let name = s.to_str()?.trim_start_matches('/').to_lowercase();
                            cmd_list.push(name);
                        }
                        Value::Table(t) => {
                            for item in t.sequence_values::<String>().flatten() {
                                let name = item.trim_start_matches('/').to_lowercase();
                                cmd_list.push(name);
                            }
                        }
                        _ => {
                            return Err(mlua::Error::RuntimeError(
                                "flow:command expects command name (string) or table of names"
                                    .into(),
                            ));
                        }
                    }

                    if let Ok(mut cmds) = coord_cmd.commands.write() {
                        cmds.push(FlowCommandMatcher {
                            commands: cmd_list,
                            action,
                        });
                    }
                    Ok(())
                },
            )?;
            flow_table.set("command", cmd_fn)?;

            // flow:on(pattern, action) OR flow.on(pattern, action)
            let coord_global = Arc::clone(&coordinator);
            let on_fn = lua_outer.create_function(
                move |_, (arg1, arg2, arg3): (Value, Option<Value>, Option<Value>)| {
                    let (pattern_val, action_val) = match (arg1, arg2, arg3) {
                        (Value::Table(_), Some(p), Some(a)) => (p, a),
                        (p, Some(a), _) => (p, a),
                        _ => {
                            return Err(mlua::Error::RuntimeError(
                                "flow:on requires pattern and action function".into(),
                            ));
                        }
                    };

                    let action: Function = match action_val {
                        Value::Function(f) => f,
                        _ => {
                            return Err(mlua::Error::RuntimeError(
                                "flow:on action must be a function".into(),
                            ));
                        }
                    };

                    let mut patterns = Vec::new();
                    match pattern_val {
                        Value::String(s) => patterns.push(s.to_str()?.to_string()),
                        Value::Table(t) => {
                            for p in t.sequence_values::<String>().flatten() {
                                patterns.push(p);
                            }
                        }
                        _ => {
                            return Err(mlua::Error::RuntimeError(
                                "flow:on pattern must be a string or array of strings".into(),
                            ));
                        }
                    }

                    if let Ok(mut list) = coord_global.globals.write() {
                        for pattern in patterns {
                            list.push(FlowGlobalMatcher {
                                pattern,
                                action: action.clone(),
                            });
                        }
                    }
                    Ok(())
                },
            )?;
            flow_table.set("on", on_fn)?;

            // flow:go_to(step_name, [chat_id]) OR flow.go_to(step_name, [chat_id])
            let coord_goto = Arc::clone(&coordinator);
            let go_to_fn = lua_outer.create_function(
                move |_, (arg1, arg2, arg3): (Value, Option<Value>, Option<Value>)| {
                    let (step_name, opt_chat) = match (arg1, arg2, arg3) {
                        (Value::Table(_), Some(s_val), chat_val) => {
                            (value_to_string(s_val)?, value_to_chat_id(chat_val))
                        }
                        (s_val, chat_val, _) => {
                            (value_to_string(s_val)?, value_to_chat_id(chat_val))
                        }
                    };

                    if let Ok(mut map) = coord_goto.current_steps.write() {
                        if let Some(chat_id) = opt_chat {
                            map.insert(chat_id, step_name);
                        } else {
                            map.insert(0, step_name.clone());
                            for v in map.values_mut() {
                                *v = step_name.clone();
                            }
                        }
                    }
                    Ok(())
                },
            )?;
            flow_table.set("go_to", go_to_fn)?;

            // flow:reset([chat_id]) OR flow.reset([chat_id])
            let coord_reset = Arc::clone(&coordinator);
            let reset_fn = lua_outer.create_function(
                move |_, (arg1, arg2): (Option<Value>, Option<Value>)| {
                    let opt_chat = match (arg1, arg2) {
                        (Some(Value::Table(_)), chat_val) => value_to_chat_id(chat_val),
                        (chat_val, _) => value_to_chat_id(chat_val),
                    };

                    let initial = coord_reset.initial_step.read().ok().and_then(|g| g.clone());
                    if let (Some(init), Ok(mut map)) = (initial, coord_reset.current_steps.write()) {
                        if let Some(chat_id) = opt_chat {
                            map.insert(chat_id, init);
                        } else {
                            map.insert(0, init.clone());
                            for v in map.values_mut() {
                                *v = init.clone();
                            }
                        }
                    }
                    Ok(())
                },
            )?;
            flow_table.set("reset", reset_fn)?;

            // flow:current_step([chat_id]) OR flow.current_step([chat_id])
            let coord_cur = Arc::clone(&coordinator);
            let cur_fn = lua_outer.create_function(
                move |lua, (arg1, arg2): (Option<Value>, Option<Value>)| {
                    let opt_chat = match (arg1, arg2) {
                        (Some(Value::Table(_)), chat_val) => value_to_chat_id(chat_val),
                        (chat_val, _) => value_to_chat_id(chat_val),
                    };
                    let chat = opt_chat.unwrap_or(0);
                    let cur = coord_cur
                        .current_steps
                        .read()
                        .ok()
                        .and_then(|m| m.get(&chat).cloned());
                    if let Some(s) = cur {
                        Ok(Value::String(lua.create_string(&s)?))
                    } else {
                        let init = coord_cur.initial_step.read().ok().and_then(|g| g.clone());
                        if let Some(i) = init {
                            Ok(Value::String(lua.create_string(&i)?))
                        } else {
                            Ok(Value::Nil)
                        }
                    }
                },
            )?;
            flow_table.set("current_step", cur_fn)?;

            // Register internal message dispatcher via ox.on_message
            let coord_disp = Arc::clone(&coordinator);
            let data_clone = ctx_data.clone();
            let on_message_reg: Function =
                lua_outer.globals().get::<Table>("ox")?.get("on_message")?;

            let filter_table = lua_outer.create_table()?;
            if let Some(ref opts) = options_val {
                if opts.contains_key("chats")? {
                    filter_table.set("chats", opts.get::<Value>("chats")?)?;
                } else if opts.contains_key("chat")? {
                    filter_table.set("chats", opts.get::<Value>("chat")?)?;
                } else if let Some(ref chats) = coordinator.target_chats {
                    if chats.len() == 1 {
                        filter_table.set("chats", chats[0])?;
                    } else {
                        let t = lua_outer.create_table()?;
                        for (i, c) in chats.iter().enumerate() {
                            t.set(i + 1, *c)?;
                        }
                        filter_table.set("chats", t)?;
                    }
                }
                if opts.contains_key("senders")? {
                    filter_table.set("senders", opts.get::<Value>("senders")?)?;
                }
                if opts.contains_key("incoming")? {
                    filter_table.set("incoming", opts.get::<bool>("incoming")?)?;
                } else if !opts.contains_key("outgoing")? {
                    filter_table.set("incoming", true)?;
                }
                if opts.contains_key("outgoing")? {
                    filter_table.set("outgoing", opts.get::<bool>("outgoing")?)?;
                }
                if opts.contains_key("private")? {
                    filter_table.set("private", opts.get::<bool>("private")?)?;
                }
                if opts.contains_key("group")? {
                    filter_table.set("group", opts.get::<bool>("group")?)?;
                }
                if opts.contains_key("channel")? {
                    filter_table.set("channel", opts.get::<bool>("channel")?)?;
                }
                if opts.contains_key("has_text")? {
                    filter_table.set("has_text", opts.get::<bool>("has_text")?)?;
                } else {
                    filter_table.set("has_text", true)?;
                }
            } else {
                filter_table.set("incoming", true)?;
                filter_table.set("has_text", true)?;
            }

            // Context builder helper
            let build_context = |lua: &Lua,
                                 coord: &Arc<FlowCoordinator>,
                                 chat_id: i64,
                                 active_step: &str,
                                 ctx_data_sub: &Table|
             -> Result<Table, mlua::Error> {
                let ctx = lua.create_table()?;
                ctx.set("step", active_step)?;
                ctx.set("data", ctx_data_sub.clone())?;

                let coord_ctx = Arc::clone(coord);
                let go_to = lua.create_function(move |_, (arg1, arg2): (Value, Option<Value>)| {
                    let target_step = match (arg1, arg2) {
                        (Value::Table(_), Some(s)) => value_to_string(s)?,
                        (s, _) => value_to_string(s)?,
                    };
                    if let Ok(mut map) = coord_ctx.current_steps.write() {
                        map.insert(chat_id, target_step);
                    }
                    Ok(())
                })?;
                ctx.set("go_to", go_to)?;

                let coord_ctx_reset = Arc::clone(coord);
                let reset = lua.create_function(move |_, _: Option<Value>| {
                    let init = coord_ctx_reset
                        .initial_step
                        .read()
                        .ok()
                        .and_then(|g| g.clone())
                        .unwrap_or_default();
                    if let Ok(mut map) = coord_ctx_reset.current_steps.write() {
                        map.insert(chat_id, init);
                    }
                    Ok(())
                })?;
                ctx.set("reset", reset)?;

                let data_get = ctx_data_sub.clone();
                let get_fn = lua.create_function(
                    move |_, (a1, a2, a3): (Value, Option<Value>, Option<Value>)| {
                        let (key, default) = match (a1, a2, a3) {
                            (Value::Table(_), Some(k), def) => (value_to_string(k)?, def),
                            (k, def, _) => (value_to_string(k)?, def),
                        };
                        let val: Value = data_get.get(key.as_str()).unwrap_or(Value::Nil);
                        if val == Value::Nil {
                            Ok(default.unwrap_or(Value::Nil))
                        } else {
                            Ok(val)
                        }
                    },
                )?;
                ctx.set("get", get_fn)?;

                let data_set = ctx_data_sub.clone();
                let set_fn = lua.create_function(
                    move |_, (a1, a2, a3): (Value, Option<Value>, Option<Value>)| {
                        let (key, val) = match (a1, a2, a3) {
                            (Value::Table(_), Some(k), Some(v)) => (value_to_string(k)?, v),
                            (k, Some(v), _) => (value_to_string(k)?, v),
                            _ => {
                                return Err(mlua::Error::RuntimeError(
                                    "ctx:set requires key and value".into(),
                                ));
                            }
                        };
                        data_set.set(key, val)?;
                        Ok(())
                    },
                )?;
                ctx.set("set", set_fn)?;

                Ok(ctx)
            };

            let dispatch_fn = lua_outer.create_async_function(move |lua, event_table: Table| {
                let coord = Arc::clone(&coord_disp);
                let ctx_data_sub = data_clone.clone();

                async move {
                    let text: String = event_table.get("text")?;
                    let chat_id: i64 = event_table.get("chat_id")?;
                    let text_lower = text.to_lowercase();

                    // 1. Check command routes
                    let (cmd_name, _) = crate::infrastructure::lua::stdlib::extract_command(&text);
                    if let Some(cmd) = cmd_name {
                        let matched_cmd = {
                            let mut found = None;
                            if let Ok(cmds) = coord.commands.read() {
                                for c in cmds.iter() {
                                    if c.commands.iter().any(|name| name == &cmd) {
                                        found = Some(c.action.clone());
                                        break;
                                    }
                                }
                            }
                            found
                        };

                        if let Some(cmd_action) = matched_cmd {
                            let active_step_name = {
                                let mut res = String::new();
                                if let Ok(mut map) = coord.current_steps.write() {
                                    if let Some(step) = map.get(&chat_id) {
                                        res = step.clone();
                                    } else {
                                        let init = coord
                                            .initial_step
                                            .read()
                                            .ok()
                                            .and_then(|g| g.clone())
                                            .unwrap_or_default();
                                        map.insert(chat_id, init.clone());
                                        res = init;
                                    }
                                }
                                res
                            };

                            let ctx = build_context(
                                &lua,
                                &coord,
                                chat_id,
                                &active_step_name,
                                &ctx_data_sub,
                            )?;
                            let result_val: Value =
                                cmd_action.call_async((event_table.clone(), ctx)).await?;

                            if let Value::String(s) = result_val {
                                let target = s.to_str()?.to_string();
                                tracing::info!(
                                    target: "oxidegram",
                                    "[FLOW] [{}] Command /{cmd} -> step {target}",
                                    coord.name
                                );
                                if let Ok(mut map) = coord.current_steps.write() {
                                    map.insert(chat_id, target);
                                }
                            }
                            return Ok(());
                        }
                    }

                    // 2. Check global matchers
                    let matched_global = {
                        let mut found = None;
                        if let Ok(globals) = coord.globals.read() {
                            for g in globals.iter() {
                                if text_lower.contains(&g.pattern.to_lowercase()) {
                                    found = Some(g.action.clone());
                                    break;
                                }
                            }
                        }
                        found
                    };

                    if let Some(global_action) = matched_global {
                        let active_step_name = {
                            let mut res = String::new();
                            if let Ok(map) = coord.current_steps.read()
                                && let Some(step) = map.get(&chat_id)
                            {
                                res = step.clone();
                            }
                            res
                        };
                        let ctx = build_context(
                            &lua,
                            &coord,
                            chat_id,
                            &active_step_name,
                            &ctx_data_sub,
                        )?;
                        let result_val: Value =
                            global_action.call_async((event_table.clone(), ctx)).await?;

                        if let Value::String(s) = result_val {
                            let target = s.to_str()?.to_string();
                            if let Ok(mut map) = coord.current_steps.write() {
                                map.insert(chat_id, target);
                            }
                        }
                        return Ok(());
                    }

                    // 3. Resolve active step
                    let active_step_name = {
                        let mut res = String::new();
                        if let Ok(mut map) = coord.current_steps.write() {
                            if let Some(step) = map.get(&chat_id) {
                                res = step.clone();
                            } else {
                                let init = coord
                                    .initial_step
                                    .read()
                                    .ok()
                                    .and_then(|g| g.clone())
                                    .unwrap_or_default();
                                map.insert(chat_id, init.clone());
                                res = init;
                            }
                        }
                        res
                    };

                    if active_step_name.is_empty() {
                        return Ok(());
                    }

                    let step_details = {
                        coord.steps.read().ok().and_then(|steps| {
                            steps.get(&active_step_name).map(|s| {
                                (
                                    s.matchers.clone(),
                                    s.action.clone(),
                                    s.next.clone(),
                                    s.on_timeout.clone(),
                                    s.timeout_secs.or(coord.default_timeout),
                                )
                            })
                        })
                    };

                    let Some((matchers, action, next, on_timeout, timeout_secs)) = step_details
                    else {
                        return Ok(());
                    };

                    // Check step timeout
                    let now = Instant::now();
                    let is_timed_out = {
                        if let (Ok(times), Some(limit)) = (coord.step_times.read(), timeout_secs) {
                            times
                                .get(&chat_id)
                                .map(|&last| now.duration_since(last).as_secs_f64() > limit)
                                .unwrap_or(false)
                        } else {
                            false
                        }
                    };

                    if is_timed_out {
                        if let Some(to_cb) = on_timeout {
                            let ctx_tbl = lua.create_table()?;
                            ctx_tbl.set("step", active_step_name.clone())?;
                            ctx_tbl.set("data", ctx_data_sub.clone())?;
                            let _ = to_cb.call_async::<()>(ctx_tbl).await;
                        }
                        // Reset to initial
                        let init = coord
                            .initial_step
                            .read()
                            .ok()
                            .and_then(|g| g.clone())
                            .unwrap_or_default();
                        if let Ok(mut map) = coord.current_steps.write() {
                            map.insert(chat_id, init);
                        }
                        return Ok(());
                    }

                    // Check matchers
                    let matches = matchers.is_empty()
                        || matchers
                            .iter()
                            .any(|m| text_lower.contains(&m.to_lowercase()));

                    if matches {
                        if let Ok(mut times) = coord.step_times.write() {
                            times.insert(chat_id, now);
                        }

                        // Build ctx table
                        let ctx = build_context(
                            &lua,
                            &coord,
                            chat_id,
                            &active_step_name,
                            &ctx_data_sub,
                        )?;

                        let result_val: Value = action.call_async((event_table, ctx)).await?;

                        let transition_step = match result_val {
                            Value::String(s) => Some(s.to_str()?.to_string()),
                            _ => next,
                        };

                        if let Some(target) = transition_step {
                            tracing::info!(target: "oxidegram", "[FLOW] [{}] Step: {} -> {}", coord.name, active_step_name, target);
                            if let Ok(mut map) = coord.current_steps.write() {
                                map.insert(chat_id, target);
                            }
                        }
                    }

                    Ok(())
                }
            })?;

            on_message_reg.call::<()>((filter_table, dispatch_fn))?;

            Ok(flow_table)
        },
    )?;

    ox_table.set("flow", flow_constructor)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flow_registration() {
        let lua = Lua::new();
        let ox = lua.create_table().unwrap();

        // Mock on_message
        ox.set(
            "on_message",
            lua.create_function(|_, (_filter, _cb): (Table, Function)| Ok(()))
                .unwrap(),
        )
        .unwrap();

        register_flow_api(&lua, &ox).unwrap();
        lua.globals().set("ox", ox).unwrap();

        let flow: Table = lua
            .load(r#"return ox.flow("test_dialog", { timeout = 30 })"#)
            .eval()
            .unwrap();

        assert_eq!(flow.get::<String>("name").unwrap(), "test_dialog");
    }

    #[test]
    fn test_flow_colon_and_dot_method_calls() {
        let lua = Lua::new();
        let ox = lua.create_table().unwrap();

        ox.set(
            "on_message",
            lua.create_function(|_, (_filter, _cb): (Table, Function)| Ok(()))
                .unwrap(),
        )
        .unwrap();

        register_flow_api(&lua, &ox).unwrap();
        lua.globals().set("ox", ox).unwrap();

        let script = r#"
            local dialog = ox.flow("rating_bot", { timeout = 60 })

            -- Method call with colon syntax (user's exact scenario)
            dialog:step("menu", {
                match = "Меню:",
                action = function(event, ctx)
                    ctx:go_to("rate")
                    return "rate"
                end
            })

            -- Method call with dot syntax
            dialog.step("rate", {
                match = "Оценить",
                action = function(event, ctx)
                    ctx.reset()
                    return "menu"
                end
            })

            -- on with colon and string
            dialog:on("stop", function(event)
                dialog:reset()
            end)

            -- on with dot and table of strings
            dialog.on({ "cancel", "exit" }, function(event)
                dialog.reset()
            end)

            -- State inspection and transition
            assert(dialog:current_step() == "menu")
            dialog:go_to("rate")
            assert(dialog:current_step() == "rate")
            dialog:reset()
            assert(dialog:current_step() == "menu")

            -- Dot syntax for navigation
            dialog.go_to("rate", 12345)
            assert(dialog.current_step(12345) == "rate")
            dialog.reset(12345)
            assert(dialog.current_step(12345) == "menu")

            return true
        "#;

        let res: bool = lua.load(script).eval().unwrap();
        assert!(res);
    }

    #[test]
    fn test_flow_commands_and_on_helpers() {
        let lua = Lua::new();
        let ox = lua.create_table().unwrap();

        ox.set(
            "on_message",
            lua.create_function(|_, (_filter, _cb): (Table, Function)| Ok(()))
                .unwrap(),
        )
        .unwrap();

        register_flow_api(&lua, &ox).unwrap();
        lua.globals().set("ox", ox).unwrap();

        let script = r#"
            local bot = ox.flow("assistant", { private = true })

            -- Command routing
            bot:command("start", function(event, ctx)
                ctx:set("started", true)
                return "onboarding"
            end)

            bot:command({ "help", "info" }, function(event, ctx)
                return "help"
            end)

            -- Global pattern / filter handler with :on
            bot:on("cancel", function(event, ctx)
                ctx:reset()
            end)

            -- Steps
            bot:step("onboarding", {
                action = function(event, ctx)
                    return "done"
                end
            })

            assert(bot.name == "assistant")
            assert(bot.data ~= nil)
            return true
        "#;

        let res: bool = lua.load(script).eval().unwrap();
        assert!(res);
    }
}
