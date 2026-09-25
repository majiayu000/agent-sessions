//! A selective, borrowed decode for statistics. Irrelevant payload bodies never
//! become Value trees; selected records use the same semantic parser as normal.
mod header;
mod usage;
use crate::parser::{Parsed, State, parse};
use crate::{AccountingPolicy, Agent, CodexUsageMode, EventKinds, LineErrorKind, ReadOptions};
use serde_json::Value;

pub(crate) enum DecodeError {
    Json(serde_json::Error),
    Fields(LineErrorKind),
}
impl From<serde_json::Error> for DecodeError {
    fn from(e: serde_json::Error) -> Self {
        Self::Json(e)
    }
}

pub(crate) fn decode(
    agent: Agent,
    bytes: &[u8],
    state: &mut State,
    opts: &ReadOptions,
) -> Result<Parsed, DecodeError> {
    if agent == Agent::Codex
        && opts.accounting == AccountingPolicy::UsageStatistics
        && !opts.include.contains(EventKinds::MESSAGE)
        && !opts.include.contains(EventKinds::TOOL_CALL)
        && !opts.include.contains(EventKinds::TOOL_RESULT)
    {
        let header: header::Header<'_> = serde_json::from_slice(bytes)?;
        if let Some((known, label)) = ignored(&header, opts.codex_usage) {
            return Ok(Parsed {
                ignored_one: Some((known, label)),
                ..Default::default()
            });
        }
        if header.kind().as_deref() == Some("event_msg")
            && opts.codex_usage == CodexUsageMode::TokenCount
            && opts.include.contains(EventKinds::USAGE)
            && header.payload.as_ref().and_then(|p| p.kind()).as_deref() == Some("token_count")
        {
            return usage::decode(&header, state, opts);
        }
        let value = header.value()?;
        return parse(
            agent,
            &value,
            state,
            opts.codex_usage,
            opts.include,
            opts.accounting,
        )
        .map_err(DecodeError::Fields);
    }
    let value: Value = serde_json::from_slice(bytes)?;
    parse(
        agent,
        &value,
        state,
        opts.codex_usage,
        opts.include,
        opts.accounting,
    )
    .map_err(DecodeError::Fields)
}

fn ignored(
    h: &header::Header<'_>,
    mode: CodexUsageMode,
) -> Option<(bool, std::borrow::Cow<'static, str>)> {
    let root_kind = h.kind()?;
    let kind = root_kind.as_ref();
    match kind {
        "session_meta" | "turn_context" => None,
        "token_usage_record" if mode == CodexUsageMode::Response => None,
        "token_usage_record"
        | "compacted"
        | "world_state"
        | "inter_agent_communication_metadata" => Some((true, known_root(kind).into())),
        "response_item" => {
            let payload_kind = h.payload.as_ref()?.kind()?;
            let kind = payload_kind.as_ref();
            let known = matches!(
                kind,
                "message"
                    | "function_call"
                    | "function_call_output"
                    | "custom_tool_call"
                    | "custom_tool_call_output"
                    | "reasoning"
                    | "web_search_call"
                    | "tool_search_call"
                    | "image_generation_call"
                    | "agent_message"
                    | "tool_search_call_output"
                    | "tool_search_output"
            );
            Some((known, label("response_item", kind, known)))
        }
        "event_msg" => {
            let payload_kind = h.payload.as_ref()?.kind()?;
            let kind = payload_kind.as_ref();
            if kind == "token_count" && mode == CodexUsageMode::TokenCount {
                return None;
            }
            let known = matches!(
                kind,
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
                    | "task_complete_notification"
            );
            Some((known, label("event_msg", kind, known)))
        }
        other => Some((false, std::borrow::Cow::Owned(other.to_owned()))),
    }
}

#[cfg(test)]
mod tests;

fn known_root(kind: &str) -> &'static str {
    match kind {
        "token_usage_record" => "token_usage_record",
        "compacted" => "compacted",
        "world_state" => "world_state",
        _ => "inter_agent_communication_metadata",
    }
}
fn label(prefix: &str, kind: &str, known: bool) -> std::borrow::Cow<'static, str> {
    if known {
        const LABELS: &[&str] = &[
            "response_item:message",
            "response_item:function_call",
            "response_item:function_call_output",
            "response_item:custom_tool_call",
            "response_item:custom_tool_call_output",
            "response_item:reasoning",
            "response_item:web_search_call",
            "response_item:tool_search_call",
            "response_item:image_generation_call",
            "response_item:agent_message",
            "response_item:tool_search_call_output",
            "response_item:tool_search_output",
            "event_msg:token_count",
            "event_msg:item_completed",
            "event_msg:task_started",
            "event_msg:task_complete",
            "event_msg:turn_aborted",
            "event_msg:thread_goal_updated",
            "event_msg:thread_settings_applied",
            "event_msg:user_message",
            "event_msg:agent_message",
            "event_msg:agent_reasoning",
            "event_msg:exec_command_begin",
            "event_msg:exec_command_end",
            "event_msg:task_complete_notification",
        ];
        if let Some(label) = LABELS
            .iter()
            .find(|label| label.split_once(':') == Some((prefix, kind)))
        {
            return std::borrow::Cow::Borrowed(label);
        }
    }
    std::borrow::Cow::Owned(format!("{prefix}:{kind}"))
}
