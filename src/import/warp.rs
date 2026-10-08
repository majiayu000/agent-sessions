//! Warp's Task protobufs, as persisted by its native agent_tasks table.
use super::*;
use crate::{
    adapters,
    parser::{required, string},
};
use prost_reflect::{DescriptorPool, DynamicMessage};
use rusqlite::Connection;
use std::sync::OnceLock;
fn decode(data: &[u8], summary: &mut ReadSummary) -> Result<Value, ImportError> {
    static POOL: OnceLock<Result<DescriptorPool, ()>> = OnceLock::new();
    let pool = POOL
        .get_or_init(|| {
            DescriptorPool::decode(include_bytes!("protocols/warp-00cdd672.pb").as_slice())
                .map_err(|_| ())
        })
        .as_ref()
        .map_err(|_| LineErrorKind::InvalidField("embedded warp descriptor".into()))?;
    let descriptor = pool
        .get_message_by_name("warp.multi_agent.v1.Task")
        .ok_or(LineErrorKind::MissingField("warp Task schema"))?;
    let m = DynamicMessage::decode(descriptor, data)
        .map_err(|_| LineErrorKind::InvalidField("warp task protobuf".into()))?;
    note_protobuf(&m, summary);
    serde_json::to_value(m).map_err(|_| LineErrorKind::InvalidField("warp task JSON".into()).into())
}
pub(super) fn import(
    conn: &Connection,
    id: &str,
    opts: &ReadOptions,
    result: &mut SessionImport,
) -> Result<(), ImportError> {
    // Session metadata contains server credentials, never project it wholesale.
    let exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM agent_conversations WHERE conversation_id=?1)",
            [id],
            |r| r.get(0),
        )
        .map_err(ReadError::Database)?;
    if !exists {
        return Err(LineErrorKind::MissingField("conversation ID").into());
    }
    let mut stmt = conn
        .prepare(
            "SELECT id,length(task),task FROM agent_tasks WHERE conversation_id=?1 ORDER BY id",
        )
        .map_err(ReadError::Database)?;
    let mut rows = stmt.query([id]).map_err(ReadError::Database)?;
    while let Some(row) = rows.next().map_err(ReadError::Database)? {
        let key: i64 = row.get(0).map_err(ReadError::Database)?;
        let size: u64 = row.get(1).map_err(ReadError::Database)?;
        if opts.max_line_bytes.is_some_and(|n| size > n as u64) {
            return Err(LineErrorKind::TooLong.into());
        }
        let data: Vec<u8> = row.get(2).map_err(ReadError::Database)?;
        let task = decode(&data, &mut result.summary)?;
        let state = State {
            session_id: Some(id.into()),
            sidechain: task
                .pointer("/dependencies/parentTaskId")
                .and_then(Value::as_str)
                .is_some_and(|v| !v.is_empty()),
            ..Default::default()
        };
        let source = ImportSource::DatabaseRow {
            table: "agent_tasks".into(),
            key: key.to_string(),
        };
        if let Some(messages) = task.get("messages").and_then(Value::as_array) {
            for m in messages {
                let mut p = Parsed {
                    include: opts.include,
                    message_id: string(m, "id"),
                    record_id: string(&task, "id"),
                    ..Default::default()
                };
                if let Some(at) = string(m, "timestamp") {
                    p.at = Some(
                        DateTime::parse_from_rfc3339(&at)
                            .map_err(|_| LineErrorKind::InvalidField("timestamp".into()))?
                            .with_timezone(&Utc),
                    );
                    p.timestamp_text = Some(at);
                }
                if let Some(user) = m.get("userQuery") {
                    adapters::message(
                        Role::User,
                        &Value::String(string(user, "query").unwrap_or_default()),
                        user,
                        &mut p,
                    )?;
                } else if let Some(agent) = m.get("agentOutput") {
                    adapters::message(
                        Role::Assistant,
                        &Value::String(string(agent, "text").unwrap_or_default()),
                        agent,
                        &mut p,
                    )?;
                } else if let Some(tool) = m.get("toolCall") {
                    if let Some((name, args)) = tool
                        .as_object()
                        .and_then(|v| v.iter().find(|(k, _)| k.as_str() != "toolCallId"))
                    {
                        adapters::call(string(tool, "toolCallId"), name, Some(args), 3, &mut p);
                    } else {
                        p.unknown.push("warp:tool-variant".into());
                    }
                } else if let Some(output) = m.get("toolCallResult") {
                    let error = output
                        .as_object()
                        .map(|v| v.values().any(|v| v.get("error").is_some()));
                    adapters::result(string(output, "toolCallId"), output, error, 3, &mut p)?;
                } else if let Some(received) = m
                    .get("messagesReceivedFromAgents")
                    .and_then(|v| v.get("messages"))
                    .and_then(Value::as_array)
                {
                    for msg in received {
                        adapters::message(
                            Role::User,
                            &Value::String(required(msg, "messageBody")?.into()),
                            msg,
                            &mut p,
                        )?;
                    }
                } else if m.as_object().is_some_and(|v| {
                    v.keys().any(|k| {
                        matches!(
                            k.as_str(),
                            "agentReasoning"
                                | "serverEvent"
                                | "systemQuery"
                                | "updateTodos"
                                | "summarization"
                                | "codeReview"
                                | "updateReviewComments"
                                | "webSearch"
                                | "webFetch"
                                | "debugOutput"
                                | "artifactEvent"
                                | "invokeSkill"
                                | "eventsFromAgents"
                                | "modelUsed"
                                | "passiveSuggestionResult"
                        )
                    })
                }) {
                    p.ignored.push("warp:system-or-reasoning".into());
                } else {
                    p.unknown.push("warp:message-variant".into());
                }
                append(p, &state, vec![source.clone()], result);
            }
        }
    }
    Ok(())
}
