//! Dialog Flow Engine and Finite State Machine (FSM) for step-by-step Lua bot dialogs.

use mlua::{Function, Lua, Table, Value};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::Instant;

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

/// State machine coordinator for a dialog flow.
struct FlowCoordinator {
    target_chats: Option<Vec<i64>>,
    initial_step: RwLock<Option<String>>,
    current_steps: RwLock<HashMap<i64, String>>,
    step_times: RwLock<HashMap<i64, Instant>>,
    default_timeout: Option<f64>,
    steps: RwLock<HashMap<String, FlowStep>>,
    globals: RwLock<Vec<FlowGlobalMatcher>>,
}

/// Registers the `ox.flow(name, [options])` constructor in Lua.
pub fn register_flow_api(lua: &Lua, ox_table: &Table) -> Result<(), mlua::Error> {
    let flow_constructor = lua.create_function(
        |lua_outer, (flow_name, options_val): (String, Option<Table>)| {
            let mut target_chats: Option<Vec<i64>> = None;
            let mut default_timeout: Option<f64> = None;
            let mut initial_step: Option<String> = None;

            if let Some(opts) = options_val {
                if let Ok(chat_id) = opts.get::<i64>("target_chat") {
                    target_chats = Some(vec![chat_id]);
                } else if let Ok(chats_table) = opts.get::<Table>("target_chats") {
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
                target_chats,
                initial_step: RwLock::new(initial_step),
                current_steps: RwLock::new(HashMap::new()),
                step_times: RwLock::new(HashMap::new()),
                default_timeout,
                steps: RwLock::new(HashMap::new()),
                globals: RwLock::new(Vec::new()),
            });

            let flow_table = lua_outer.create_table()?;
            flow_table.set("name", flow_name.clone())?;

            // Shared context data table for this flow
            let ctx_data = lua_outer.create_table()?;
            flow_table.set("data", ctx_data.clone())?;

            // flow:step(name, config)
            let coord_step = Arc::clone(&coordinator);
            let step_fn =
                lua_outer.create_function(move |_, (step_name, config): (String, Table)| {
                    let action: Function = config.get("action")?;
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
                })?;
            flow_table.set("step", step_fn)?;

            // flow:on_match(pattern, action)
            let coord_global = Arc::clone(&coordinator);
            let on_match_fn =
                lua_outer.create_function(move |_, (pattern, action): (String, Function)| {
                    if let Ok(mut list) = coord_global.globals.write() {
                        list.push(FlowGlobalMatcher { pattern, action });
                    }
                    Ok(())
                })?;
            flow_table.set("on_match", on_match_fn)?;

            // flow:go_to(step_name, [chat_id])
            let coord_goto = Arc::clone(&coordinator);
            let go_to_fn = lua_outer.create_function(
                move |_, (step_name, opt_chat): (String, Option<i64>)| {
                    if let Ok(mut map) = coord_goto.current_steps.write() {
                        if let Some(chat_id) = opt_chat {
                            map.insert(chat_id, step_name);
                        } else {
                            for v in map.values_mut() {
                                *v = step_name.clone();
                            }
                        }
                    }
                    Ok(())
                },
            )?;
            flow_table.set("go_to", go_to_fn)?;

            // flow:reset([chat_id])
            let coord_reset = Arc::clone(&coordinator);
            let reset_fn = lua_outer.create_function(move |_, opt_chat: Option<i64>| {
                let initial = coord_reset.initial_step.read().ok().and_then(|g| g.clone());
                if let (Some(init), Ok(mut map)) = (initial, coord_reset.current_steps.write()) {
                    if let Some(chat_id) = opt_chat {
                        map.insert(chat_id, init);
                    } else {
                        for v in map.values_mut() {
                            *v = init.clone();
                        }
                    }
                }
                Ok(())
            })?;
            flow_table.set("reset", reset_fn)?;

            // flow:current_step([chat_id]) -> string
            let coord_cur = Arc::clone(&coordinator);
            let cur_fn = lua_outer.create_function(move |lua, opt_chat: Option<i64>| {
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
            })?;
            flow_table.set("current_step", cur_fn)?;

            // Register internal message dispatcher via ox.on_message
            let coord_disp = Arc::clone(&coordinator);
            let data_clone = ctx_data.clone();
            let on_message_reg: Function =
                lua_outer.globals().get::<Table>("ox")?.get("on_message")?;

            let filter_table = lua_outer.create_table()?;
            if let Some(ref chats) = coordinator.target_chats {
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
            filter_table.set("incoming", true)?;
            filter_table.set("has_text", true)?;

            let dispatch_fn = lua_outer.create_async_function(move |lua, event_table: Table| {
                let coord = Arc::clone(&coord_disp);
                let ctx_data_sub = data_clone.clone();

                async move {
                    let text: String = event_table.get("text")?;
                    let chat_id: i64 = event_table.get("chat_id")?;
                    let text_lower = text.to_lowercase();

                    // 1. Check global matchers
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
                        let _ = global_action.call_async::<()>(event_table).await;
                        return Ok(());
                    }

                    // 2. Resolve active step
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
                        let ctx = lua.create_table()?;
                        ctx.set("step", active_step_name)?;
                        ctx.set("data", ctx_data_sub)?;

                        let coord_ctx = Arc::clone(&coord);
                        let go_to = lua.create_function(move |_, target_step: String| {
                            if let Ok(mut map) = coord_ctx.current_steps.write() {
                                map.insert(chat_id, target_step);
                            }
                            Ok(())
                        })?;
                        ctx.set("go_to", go_to)?;

                        let coord_ctx_reset = Arc::clone(&coord);
                        let reset = lua.create_function(move |_, ()| {
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

                        let result_val: Value = action.call_async((event_table, ctx)).await?;

                        let transition_step = match result_val {
                            Value::String(s) => Some(s.to_str()?.to_string()),
                            _ => next,
                        };

                        if let (Some(target), Ok(mut map)) =
                            (transition_step, coord.current_steps.write())
                        {
                            map.insert(chat_id, target);
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
}
