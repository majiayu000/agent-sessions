pub(crate) mod origin;
mod tools;
pub(crate) mod usage;
use crate::parser::{Parsed, State, required, string, text_projection};
use crate::{CodexUsageMode, Event, EventKinds, LineErrorKind, Message, MetaUpdate, Origin, Role};
use serde_json::Value;

pub(crate) fn parse(
    v: &Value,
    state: &mut State,
    p: &mut Parsed,
    mode: CodexUsageMode,
) -> Result<(), LineErrorKind> {
    let kind = required(v, "type")?;
    match kind {
        "session_meta" | "turn_context" => {
            let payload = v
                .get("payload")
                .filter(|v| v.is_object())
                .ok_or(LineErrorKind::MissingField("payload"))?;
            let session_id = if kind == "session_meta" {
                state.codex_paginated = match payload.get("history_mode") {
                    None => false,
                    Some(Value::String(mode)) if mode == "legacy" => false,
                    Some(Value::String(mode)) if mode == "paginated" => true,
                    Some(Value::String(mode)) => {
                        p.unknown.push(format!("history_mode:{mode}"));
                        false
                    }
                    _ => return Err(LineErrorKind::InvalidField("history_mode".into())),
                };
                string(payload, "id")
            } else {
                None
            };
            if let Some(id) = &session_id {
                if state.session_id.as_ref().is_some_and(|old| old != id) {
                    state.previous = None;
                    state.broken_usage = false;
                    state.model = None;
                    state.sidechain = false;
                }
                state.session_id = Some(id.clone());
            }
            let model = model(payload);
            if model.is_some() {
                state.model.clone_from(&model);
            }
            state.sidechain |= origin::classify(payload) == Origin::Subagent;
            p.emit(
                0,
                Event::Meta(MetaUpdate {
                    session_id,
                    model,
                    cwd: string(payload, "cwd"),
                    git_branch: payload
                        .pointer("/git/branch")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                    agent_version: string(payload, "cli_version"),
                    origin: Some(origin::classify(payload)),
                    source: payload.get("source").cloned(),
                    originator: string(payload, "originator"),
                    thread_source: string(payload, "thread_source"),
                }),
            );
        }
        "response_item" => {
            let payload = v
                .get("payload")
                .ok_or(LineErrorKind::MissingField("payload"))?;
            // Early rollouts omitted payload.type for assistant text.
            let legacy = payload.get("type").is_none() && payload.get("content").is_some();
            let ty = if legacy {
                "message"
            } else {
                required(payload, "type")?
            };
            p.message_id = string(payload, "id");
            if state.codex_completed() {
                p.ignored.push(format!("response_item:{ty}"));
                return Ok(());
            }
            match ty {
                "message" if p.wants(EventKinds::MESSAGE) || p.wants(EventKinds::CONTENT) => {
                    let r = if legacy {
                        "assistant"
                    } else {
                        required(payload, "role")?
                    };
                    let Some(role) = Role::parse(r) else {
                        p.unknown.push(format!("role:{r}"));
                        return Ok(());
                    };
                    let c = payload
                        .get("content")
                        .ok_or(LineErrorKind::MissingField("content"))?;
                    let (body, text_segments) = text_projection(c, p, Some(role))?;
                    p.emit(
                        1,
                        Event::Message(Message {
                            role,
                            text: body,
                            text_segments,
                            is_meta: false,
                            is_sidechain: false,
                            parent_id: None,
                        }),
                    );
                }
                "function_call" | "custom_tool_call" if p.wants(EventKinds::TOOL_CALL) => {
                    tools::parse(ty, payload, p)?
                }
                "function_call_output" | "custom_tool_call_output"
                    if p.wants(EventKinds::TOOL_RESULT) =>
                {
                    tools::parse(ty, payload, p)?
                }
                "message"
                | "function_call"
                | "custom_tool_call"
                | "function_call_output"
                | "custom_tool_call_output" => p.ignored.push(format!("response_item:{ty}")),
                "reasoning"
                | "image_generation_call"
                | "agent_message"
                | "compaction"
                | "compaction_summary" => {
                    crate::adapters::native_content(Some(Role::Assistant), ty, payload, 1, p);
                }
                "web_search_call" => {
                    crate::adapters::call(
                        string(payload, "id"),
                        "web_search",
                        payload.get("action"),
                        3,
                        p,
                    );
                    if matches!(
                        payload.get("status").and_then(Value::as_str),
                        Some("completed" | "failed")
                    ) {
                        crate::adapters::result(
                            string(payload, "id"),
                            payload,
                            Some(payload["status"] == "failed"),
                            4,
                            p,
                        )?;
                    }
                }
                "tool_search_call" => {
                    crate::adapters::call(
                        string(payload, "call_id").or_else(|| string(payload, "id")),
                        "tool_search",
                        payload.get("arguments"),
                        3,
                        p,
                    );
                }
                "tool_search_output" => {
                    crate::adapters::result(
                        string(payload, "call_id").or_else(|| string(payload, "id")),
                        payload,
                        payload
                            .get("status")
                            .and_then(Value::as_str)
                            .map(|status| status == "failed"),
                        3,
                        p,
                    )?;
                }
                other => p.unknown.push(format!("response_item:{other}")),
            }
        }
        "event_msg" => {
            let payload = v
                .get("payload")
                .ok_or(LineErrorKind::MissingField("payload"))?;
            let ty = required(payload, "type")?;
            match ty {
                "item_completed" if state.codex_completed() => {
                    completed(payload, p)?;
                }
                "token_count"
                    if mode == CodexUsageMode::TokenCount && p.wants(EventKinds::USAGE) =>
                {
                    if p.accounting == crate::AccountingPolicy::UsageStatistics
                        && payload.get("info").is_some_and(|v| !v.is_null())
                        && v.get("timestamp").and_then(Value::as_str).is_none()
                    {
                        return Err(LineErrorKind::MissingField("timestamp"));
                    }
                    if p.accounting == crate::AccountingPolicy::UsageStatistics
                        && usage::has_unaccounted_last(payload)
                    {
                        p.ignored.push(crate::CODEX_MISSING_TOTAL_USAGE.into());
                    }
                    let observed_model = model(payload);
                    let mut usage = usage::token_count(payload, state, p.accounting)?;
                    usage::apply_model(usage.as_mut(), state, observed_model);
                    if let Some(u) = usage {
                        p.emit(2, Event::Usage(u));
                    }
                }
                "token_count"
                | "item_completed"
                | "task_started"
                | "task_complete"
                | "turn_aborted"
                | "thread_goal_updated"
                | "thread_settings_applied"
                | "user_message"
                | "agent_message"
                | "agent_reasoning"
                | "exec_command_begin"
                | "exec_command_end"
                | "task_complete_notification" => p.ignored.push(format!("event_msg:{ty}")),
                other => p.unknown.push(format!("event_msg:{other}")),
            }
        }
        "token_usage_record" if mode == CodexUsageMode::Response && p.wants(EventKinds::USAGE) => {
            let payload = v
                .get("payload")
                .ok_or(LineErrorKind::MissingField("payload"))?;
            p.emit(2, Event::Usage(usage::response(payload, state)?));
        }
        "user_message" if p.wants(EventKinds::MESSAGE) => {
            if !state.codex_completed() {
                crate::adapters::message(
                    Role::User,
                    v.get("content")
                        .ok_or(LineErrorKind::MissingField("content"))?,
                    v,
                    p,
                )?;
            } else {
                p.ignored.push(kind.into());
            }
        }
        "user_message"
        | "token_usage_record"
        | "compacted"
        | "world_state"
        | "inter_agent_communication_metadata" => p.ignored.push(kind.into()),
        other => p.unknown.push(other.into()),
    }
    Ok(())
}

fn completed(payload: &Value, p: &mut Parsed) -> Result<(), LineErrorKind> {
    let item = payload
        .get("item")
        .ok_or(LineErrorKind::MissingField("item"))?;
    let kind = required(item, "type")?;
    p.message_id = string(item, "id");
    match kind {
        "UserMessage" | "AgentMessage" => {
            if p.wants(EventKinds::MESSAGE) || p.wants(EventKinds::CONTENT) {
                crate::adapters::message(
                    if kind == "UserMessage" {
                        Role::User
                    } else {
                        Role::Assistant
                    },
                    item.get("content")
                        .ok_or(LineErrorKind::MissingField("content"))?,
                    item,
                    p,
                )?;
            }
        }
        "Plan" if p.wants(EventKinds::MESSAGE) => {
            crate::adapters::message(
                Role::Assistant,
                item.get("text")
                    .ok_or(LineErrorKind::MissingField("text"))?,
                item,
                p,
            )?;
        }
        "HookPrompt" if p.wants(EventKinds::MESSAGE) || p.wants(EventKinds::CONTENT) => {
            crate::adapters::message(
                Role::System,
                item.get("fragments")
                    .ok_or(LineErrorKind::MissingField("fragments"))?,
                item,
                p,
            )?;
        }
        "FunctionCallOutput" => {
            crate::adapters::result(
                string(item, "id"),
                item.get("output")
                    .ok_or(LineErrorKind::MissingField("output"))?,
                None,
                3,
                p,
            )?;
        }
        "FileChange" => {
            // The projected item retains changes, not the original apply_patch arguments.
            crate::adapters::call(string(item, "id"), "apply_patch", None, 3, p);
            crate::adapters::result(
                string(item, "id"),
                item,
                item.get("status")
                    .and_then(Value::as_str)
                    .map(|s| matches!(s, "failed" | "declined")),
                4,
                p,
            )?;
            if let Some((_, Event::ToolResult(result))) = p
                .events
                .iter_mut()
                .find(|(_, e)| matches!(e, Event::ToolResult(_)))
            {
                result.text = [string(item, "stdout"), string(item, "stderr")]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>()
                    .join("\n");
            }
        }
        "CollabAgentToolCall" => {
            crate::adapters::call(string(item, "id"), required(item, "tool")?, None, 3, p);
            crate::adapters::result(
                string(item, "id"),
                item,
                item.get("status")
                    .and_then(Value::as_str)
                    .map(|s| matches!(s, "failed" | "interrupted")),
                4,
                p,
            )?;
        }
        "WebSearch" => web_search(item, p)?,
        "Extension" => match required(item, "kind")? {
            "web.search" => web_search(item, p)?,
            "clock.sleep" => {
                crate::adapters::call(string(item, "id"), "clock.sleep", None, 3, p);
                crate::adapters::result(string(item, "id"), item, None, 4, p)?;
            }
            "image_gen.generation" => crate::adapters::native_content(
                Some(Role::Assistant),
                "image_gen.generation",
                item,
                1,
                p,
            ),
            other => p.unknown.push(format!("item_completed:extension:{other}")),
        },
        "CommandExecution" => {
            if p.wants(EventKinds::TOOL_CALL) {
                crate::adapters::call(
                    string(item, "id"),
                    "exec_command",
                    item.get("command"),
                    3,
                    p,
                );
            }
            if let Some(output) = item.get("aggregated_output").filter(|v| !v.is_null()) {
                crate::adapters::result(
                    string(item, "id"),
                    output,
                    item.get("exit_code")
                        .and_then(Value::as_i64)
                        .map(|v| v != 0),
                    4,
                    p,
                )?;
            }
        }
        "McpToolCall" | "DynamicToolCall" => {
            if p.wants(EventKinds::TOOL_CALL) {
                crate::adapters::call(
                    string(item, "id"),
                    required(item, "tool")?,
                    item.get("arguments"),
                    3,
                    p,
                );
            }
            if let Some(output) = item
                .pointer("/result/content")
                .or_else(|| item.get("content_items"))
                .filter(|v| !v.is_null())
            {
                crate::adapters::result(
                    string(item, "id"),
                    output,
                    item.get("success")
                        .and_then(Value::as_bool)
                        .map(|v| !v)
                        .or_else(|| item.pointer("/result/isError").and_then(Value::as_bool)),
                    4,
                    p,
                )?;
            } else if let Some(error) = item.get("error").filter(|v| !v.is_null()) {
                crate::adapters::result(string(item, "id"), error, Some(true), 4, p)?;
            }
        }
        "Reasoning" | "ContextCompaction" | "SubAgentActivity" | "ImageView"
        | "ImageGeneration" => {
            crate::adapters::native_content(Some(Role::Assistant), kind, item, 1, p);
        }
        "Plan" | "HookPrompt" => p.ignored.push(format!("item_completed:{kind}")),
        other => p.unknown.push(format!("item_completed:{other}")),
    }
    Ok(())
}

fn web_search(item: &Value, p: &mut Parsed) -> Result<(), LineErrorKind> {
    // Search projection retains the query/action and opaque search results.
    crate::adapters::call(string(item, "id"), "web_search", item.get("action"), 3, p);
    crate::adapters::result(string(item, "id"), item, None, 4, p)?;
    Ok(())
}

fn model(payload: &Value) -> Option<String> {
    [
        payload.pointer("/info/model"),
        payload.pointer("/info/model_name"),
        payload.pointer("/info/metadata/model"),
        payload.get("model"),
    ]
    .into_iter()
    .flatten()
    .filter_map(Value::as_str)
    .find(|s| !s.trim().is_empty())
    .map(str::to_owned)
}
