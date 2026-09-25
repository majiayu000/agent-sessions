mod tools;
mod usage;
use crate::parser::{Parsed, State, flag, required, string, text};
use crate::{Event, EventKinds, LineErrorKind, Message, MetaUpdate, Role};
use serde_json::Value;

pub(crate) fn parse(v: &Value, state: &mut State, p: &mut Parsed) -> Result<(), LineErrorKind> {
    let kind = required(v, "type")?;
    let session_id = string(v, "sessionId").or_else(|| string(v, "session_id"));
    if let Some(id) = &session_id {
        state.session_id = Some(id.clone());
    }
    let model = v
        .pointer("/message/model")
        .and_then(Value::as_str)
        .map(str::to_owned);
    if model.is_some() {
        state.model.clone_from(&model);
    }
    let meta = MetaUpdate {
        session_id,
        model,
        cwd: string(v, "cwd"),
        git_branch: string(v, "gitBranch"),
        agent_version: string(v, "version"),
        ..Default::default()
    };
    if meta != MetaUpdate::default() {
        p.emit(0, Event::Meta(meta));
    }
    p.message_id = v
        .pointer("/message/id")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| string(v, "uuid"));
    match kind {
        "user" | "assistant" => {
            let m = v
                .get("message")
                .filter(|m| m.is_object())
                .ok_or(LineErrorKind::MissingField("message"))?;
            if p.wants(EventKinds::MESSAGE) {
                let c = m
                    .get("content")
                    .ok_or(LineErrorKind::MissingField("content"))?;
                let body = text(c, p)?;
                let is_meta = flag(
                    v.get("isMeta")
                        .or_else(|| v.get("is_meta"))
                        .or_else(|| m.get("isMeta"))
                        .or_else(|| m.get("is_meta")),
                    "isMeta",
                )?;
                p.emit(
                    1,
                    Event::Message(Message {
                        role: if kind == "user" {
                            Role::User
                        } else {
                            Role::Assistant
                        },
                        text: body,
                        is_meta,
                        is_sidechain: flag(v.get("isSidechain"), "isSidechain")?,
                        parent_id: string(v, "parentUuid"),
                    }),
                );
            }
            if (p.wants(EventKinds::TOOL_CALL) || p.wants(EventKinds::TOOL_RESULT))
                && let Some(c) = m.get("content")
            {
                tools::parse(c, p)?;
            }
            if p.wants(EventKinds::USAGE)
                && let Some(u) = m.get("usage").filter(|u| !u.is_null())
            {
                p.emit(2, Event::Usage(usage::parse(m, u)?));
            }
        }
        "progress" => {
            if let Some(m) = v.pointer("/data/message/message") {
                p.message_id = string(m, "id").or_else(|| {
                    v.pointer("/data/message/id")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                });
                if let Some(c) = m.get("content") {
                    tools::parse(c, p)?;
                }
            }
            p.ignored.push("progress".into());
        }
        "system" if p.wants(EventKinds::MESSAGE) => {
            if let Some(c) = v.pointer("/message/content").or_else(|| v.get("content")) {
                let body = text(c, p)?;
                p.emit(
                    1,
                    Event::Message(Message {
                        role: Role::System,
                        text: body,
                        is_meta: false,
                        is_sidechain: false,
                        parent_id: None,
                    }),
                );
            } else {
                p.ignored.push(kind.into());
            }
        }
        "system"
        | "attachment"
        | "summary"
        | "last-prompt"
        | "mode"
        | "permission-mode"
        | "ai-title"
        | "atis-latch"
        | "queue-operation"
        | "pr-link"
        | "file-history-snapshot"
        | "file-history-delta"
        | "custom-title"
        | "agent-name"
        | "cost-state"
        | "started"
        | "result"
        | "frame-link" => p.ignored.push(kind.into()),
        other => p.unknown.push(other.into()),
    }
    Ok(())
}
