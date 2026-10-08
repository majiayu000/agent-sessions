//! Cursor Agent's content-addressed protobuf checkpoint. Only referenced blobs
//! are read, in vendor checkpoint order; unreachable blobs are not history.
use super::*;
use crate::{
    adapters,
    parser::{required, string},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use prost_reflect::{DescriptorPool, DynamicMessage};
use rusqlite::Connection;
use std::sync::OnceLock;

fn invalid(name: &str) -> LineErrorKind {
    LineErrorKind::InvalidField(name.into())
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn unhex(value: &str) -> Result<Vec<u8>, LineErrorKind> {
    if !value.len().is_multiple_of(2) {
        return Err(invalid("meta hex"));
    }
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|b| {
            let s = std::str::from_utf8(b).map_err(|_| invalid("meta hex"))?;
            u8::from_str_radix(s, 16).map_err(|_| invalid("meta hex"))
        })
        .collect()
}
fn bytes(
    conn: &Connection,
    table: &str,
    id: &str,
    opts: &ReadOptions,
) -> Result<Vec<u8>, ImportError> {
    let sql = match table {
        "meta" => "SELECT length(CAST(value AS BLOB)),CAST(value AS BLOB) FROM meta WHERE key=?1",
        _ => "SELECT length(data),data FROM blobs WHERE id=?1",
    };
    let mut stmt = conn.prepare(sql).map_err(ReadError::Database)?;
    let mut rows = stmt.query([id]).map_err(ReadError::Database)?;
    let row = rows
        .next()
        .map_err(ReadError::Database)?
        .ok_or(LineErrorKind::MissingField("referenced blob"))?;
    let length: u64 = row.get(0).map_err(ReadError::Database)?;
    if opts
        .max_line_bytes
        .is_some_and(|limit| length > limit as u64)
    {
        return Err(LineErrorKind::TooLong.into());
    }
    row.get(1).map_err(|e| ReadError::Database(e).into())
}
pub(super) fn metadata(conn: &Connection, opts: &ReadOptions) -> Result<Value, ImportError> {
    let data = bytes(conn, "meta", "0", opts)?;
    let encoded = std::str::from_utf8(&data).map_err(|_| LineErrorKind::InvalidUtf8)?;
    serde_json::from_slice(&unhex(encoded)?).map_err(|_| LineErrorKind::InvalidJson.into())
}
fn decode(name: &str, data: &[u8], summary: &mut ReadSummary) -> Result<Value, ImportError> {
    static POOL: OnceLock<Result<DescriptorPool, ()>> = OnceLock::new();
    let pool = POOL.get_or_init(|| {
        DescriptorPool::decode(include_bytes!("protocols/cursor-agent-2026.10.01.pb").as_slice())
            .map_err(|_| ())
    });
    let pool = pool
        .as_ref()
        .map_err(|_| invalid("embedded cursor descriptor"))?;
    let descriptor = pool
        .get_message_by_name(name)
        .ok_or_else(|| invalid("cursor protobuf type"))?;
    let message =
        DynamicMessage::decode(descriptor, data).map_err(|_| invalid("cursor protobuf data"))?;
    note_protobuf(&message, summary);
    serde_json::to_value(message).map_err(|_| invalid("cursor protobuf JSON").into())
}
fn referenced(
    conn: &Connection,
    reference: &Value,
    name: &str,
    opts: &ReadOptions,
    summary: &mut ReadSummary,
) -> Result<(Value, ImportSource), ImportError> {
    let reference = reference
        .as_str()
        .ok_or_else(|| invalid("blob reference"))?;
    let id = hex(&STANDARD
        .decode(reference)
        .map_err(|_| invalid("blob reference"))?);
    let value = decode(name, &bytes(conn, "blobs", &id, opts)?, summary)?;
    Ok((
        value,
        ImportSource::DatabaseRow {
            table: "blobs".into(),
            key: id,
        },
    ))
}
fn array<'a>(v: &'a Value, key: &str) -> Result<&'a [Value], ImportError> {
    match v.get(key) {
        None => Ok(&[]),
        Some(v) => v
            .as_array()
            .map(Vec::as_slice)
            .ok_or_else(|| invalid(key).into()),
    }
}
pub(super) fn import(
    conn: &Connection,
    id: &str,
    opts: &ReadOptions,
    result: &mut SessionImport,
) -> Result<(), ImportError> {
    let meta = metadata(conn, opts)?;
    if required(&meta, "agentId")? != id {
        return Err(invalid("agent ID does not match store").into());
    }
    let root_id = required(&meta, "latestRootBlobId")?;
    let root = decode(
        "agent.v1.ConversationStateStructure",
        &bytes(conn, "blobs", root_id, opts)?,
        &mut result.summary,
    )?;
    let sources = vec![
        ImportSource::DatabaseRow {
            table: "meta".into(),
            key: "0".into(),
        },
        ImportSource::DatabaseRow {
            table: "blobs".into(),
            key: root_id.into(),
        },
    ];
    let state = State {
        session_id: Some(id.into()),
        model: string(&meta, "lastUsedModel"),
        ..Default::default()
    };
    emit_meta(
        MetaUpdate {
            session_id: state.session_id.clone(),
            model: state.model.clone(),
            ..Default::default()
        },
        sources.clone(),
        opts,
        result,
    );
    for reference in array(&root, "rootPromptMessagesJson")? {
        let key = hex(&STANDARD
            .decode(
                reference
                    .as_str()
                    .ok_or_else(|| invalid("prompt reference"))?,
            )
            .map_err(|_| invalid("prompt reference"))?);
        let native: Value = serde_json::from_slice(&bytes(conn, "blobs", &key, opts)?)
            .map_err(|_| LineErrorKind::InvalidJson)?;
        let mut p = Parsed {
            include: opts.include,
            ..Default::default()
        };
        adapters::cursor_cli_prompt(&native, &mut p)?;
        let mut src = sources.clone();
        src.push(ImportSource::DatabaseRow {
            table: "blobs".into(),
            key,
        });
        append(p, &state, src, result);
    }
    for reference in array(&root, "turns")? {
        let (turn, turn_source) = referenced(
            conn,
            reference,
            "agent.v1.ConversationTurnStructure",
            opts,
            &mut result.summary,
        )?;
        let mut src = sources.clone();
        src.push(turn_source);
        if let Some(turn) = turn.get("agentConversationTurn") {
            let (user, user_source) = referenced(
                conn,
                turn.get("userMessage")
                    .ok_or(LineErrorKind::MissingField("userMessage"))?,
                "agent.v1.UserMessage",
                opts,
                &mut result.summary,
            )?;
            let mut p = Parsed {
                include: opts.include,
                message_id: string(&user, "messageId"),
                ..Default::default()
            };
            if p.wants(EventKinds::MESSAGE) || p.wants(EventKinds::CONTENT) {
                adapters::message(
                    Role::User,
                    &Value::String(string(&user, "text").unwrap_or_default()),
                    &user,
                    &mut p,
                )?;
            }
            adapters::native_content(Some(Role::User), "userMessage", &user, 3, &mut p);
            let mut user_src = src.clone();
            user_src.push(user_source);
            append(p, &state, user_src, result);
            for reference in array(turn, "steps")? {
                let (step, step_source) = referenced(
                    conn,
                    reference,
                    "agent.v1.ConversationStep",
                    opts,
                    &mut result.summary,
                )?;
                let mut step_src = src.clone();
                step_src.push(step_source);
                let mut p = Parsed {
                    include: opts.include,
                    ..Default::default()
                };
                if let Some(m) = step.get("assistantMessage") {
                    p.at = adapters::millis(
                        m.get("startedAtMs")
                            .and_then(Value::as_str)
                            .and_then(|s| s.parse::<i64>().ok())
                            .map(Value::from)
                            .as_ref(),
                    )?;
                    if p.wants(EventKinds::MESSAGE) || p.wants(EventKinds::CONTENT) {
                        adapters::message(
                            Role::Assistant,
                            &Value::String(string(m, "text").unwrap_or_default()),
                            m,
                            &mut p,
                        )?;
                    }
                } else if step.get("thinkingMessage").is_some() {
                    adapters::native_content(
                        Some(Role::Assistant),
                        "thinkingMessage",
                        &step["thinkingMessage"],
                        1,
                        &mut p,
                    );
                } else if let Some(tool) = step.get("toolCall") {
                    if let Some((name, call)) = tool
                        .as_object()
                        .and_then(|m| m.iter().find(|(name, _)| name.ends_with("ToolCall")))
                    {
                        let id = string(tool, "toolCallId")
                            .or_else(|| call.get("args").and_then(|v| string(v, "toolCallId")));
                        adapters::call(id.clone(), name, call.get("args"), 0, &mut p);
                        if let Some(output) = call.get("result") {
                            adapters::result(
                                id,
                                output,
                                output
                                    .get("error")
                                    .map(|_| true)
                                    .or_else(|| output.get("success").map(|_| false)),
                                1,
                                &mut p,
                            )?;
                            if let Some(text) = output
                                .pointer("/success/content")
                                .or_else(|| output.pointer("/success/stdout"))
                                .or_else(|| output.pointer("/success/markdown"))
                                .and_then(Value::as_str)
                            {
                                for (_, event) in &mut p.events {
                                    if let Event::ToolResult(r) = event {
                                        r.text = text.into();
                                    }
                                }
                            }
                            if p.wants(EventKinds::TOOL_RESULT)
                                && let Some(reference) = output.pointer("/success/contentBlobId")
                            {
                                let key = hex(&STANDARD
                                    .decode(
                                        reference
                                            .as_str()
                                            .ok_or_else(|| invalid("content blob reference"))?,
                                    )
                                    .map_err(|_| invalid("content blob reference"))?);
                                let text = String::from_utf8(bytes(conn, "blobs", &key, opts)?)
                                    .map_err(|_| LineErrorKind::InvalidUtf8)?;
                                for (_, event) in &mut p.events {
                                    if let Event::ToolResult(r) = event {
                                        r.text = text.clone();
                                    }
                                }
                                step_src.push(ImportSource::DatabaseRow {
                                    table: "blobs".into(),
                                    key,
                                });
                            }
                        }
                    } else {
                        p.unknown.push("cursor-cli:tool-variant".into());
                    }
                } else {
                    p.unknown.push("cursor-cli:step-variant".into());
                }
                append(p, &state, step_src, result);
            }
        } else if let Some(shell) = turn.get("shellConversationTurn") {
            let (command, command_source) = referenced(
                conn,
                shell
                    .get("shellCommand")
                    .ok_or(LineErrorKind::MissingField("shellCommand"))?,
                "agent.v1.ShellCommand",
                opts,
                &mut result.summary,
            )?;
            let mut p = Parsed {
                include: opts.include,
                ..Default::default()
            };
            adapters::call(None, "shellCommand", Some(&command), 3, &mut p);
            src.push(command_source);
            if let Some(reference) = shell.get("shellOutput") {
                let (output, output_source) = referenced(
                    conn,
                    reference,
                    "agent.v1.ShellOutput",
                    opts,
                    &mut result.summary,
                )?;
                adapters::result(
                    None,
                    &output,
                    Some(output.get("exitCode").and_then(Value::as_i64).unwrap_or(0) != 0),
                    4,
                    &mut p,
                )?;
                if let Some((_, Event::ToolResult(r))) = p.events.last_mut() {
                    r.text = string(&output, "stdout").unwrap_or_default();
                }
                src.push(output_source);
            }
            append(p, &state, src, result);
        } else {
            tally(&mut result.summary.unknown_types, "cursor-cli:turn-variant");
        }
    }
    tally(
        &mut result.summary.ignored_types,
        "cursor-cli:context-token-count-not-billing",
    );
    Ok(())
}
