mod origin;
mod tools;
mod usage;
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
            let ty = required(payload, "type")?;
            p.message_id = string(payload, "id");
            match ty {
                "message" if p.wants(EventKinds::MESSAGE) => {
                    let r = required(payload, "role")?;
                    let Some(role) = Role::parse(r) else {
                        p.unknown.push(format!("role:{r}"));
                        return Ok(());
                    };
                    let c = payload
                        .get("content")
                        .ok_or(LineErrorKind::MissingField("content"))?;
                    let (body, text_segments) = text_projection(c, p)?;
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
                | "web_search_call"
                | "tool_search_call"
                | "image_generation_call"
                | "agent_message"
                | "tool_search_call_output"
                | "tool_search_output" => p.ignored.push(format!("response_item:{ty}")),
                other => p.unknown.push(format!("response_item:{other}")),
            }
        }
        "event_msg" => {
            let payload = v
                .get("payload")
                .ok_or(LineErrorKind::MissingField("payload"))?;
            let ty = required(payload, "type")?;
            match ty {
                "token_count"
                    if mode == CodexUsageMode::TokenCount && p.wants(EventKinds::USAGE) =>
                {
                    let observed_model = model(payload);
                    if let Some(mut u) = usage::token_count(payload, state, p.accounting)? {
                        if let Some(model) = observed_model {
                            state.model = Some(model.clone());
                            u.model = Some(model);
                        }
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
        "token_usage_record"
        | "compacted"
        | "world_state"
        | "inter_agent_communication_metadata" => p.ignored.push(kind.into()),
        other => p.unknown.push(other.into()),
    }
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
