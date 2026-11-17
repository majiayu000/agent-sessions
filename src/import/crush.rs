//! Crush's per-project `.crush/crush.db` (charmbracelet/crush, goose migrations).
//!
//! `messages.parts` is a JSON array of `{"type", "data"}` parts. Tool results
//! are separate `role='tool'` rows. Session token columns hold the last step's
//! values (overwritten, not summed) and messages carry no usage, so no Usage
//! events are produced. `created_at` is in seconds; rows within one second keep
//! their insertion order through `rowid`.
use super::*;
use crate::adapters;
use rusqlite::Connection;

pub(super) fn list(conn: &Connection) -> Result<Vec<DatabaseSession>, ImportError> {
    let mut stmt = conn
        .prepare("SELECT id,title FROM sessions ORDER BY created_at,rowid")
        .map_err(ReadError::Database)?;
    let rows = stmt
        .query_map([], |r| {
            Ok(DatabaseSession {
                id: r.get(0)?,
                title: r.get(1)?,
                cwd: None,
            })
        })
        .map_err(ReadError::Database)?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| ReadError::Database(e).into())
}

pub(super) fn import(
    conn: &Connection,
    id: &str,
    opts: &ReadOptions,
    result: &mut SessionImport,
) -> Result<(), ImportError> {
    let parent: Option<String> = conn
        .query_row(
            "SELECT parent_session_id FROM sessions WHERE id=?1",
            [id],
            |r| r.get(0),
        )
        .map_err(ReadError::Database)?;
    // Last-step snapshot, not a ledger: see the module comment.
    tally(
        &mut result.summary.ignored_types,
        "crush:session-token-snapshot",
    );
    let state = State {
        session_id: Some(id.into()),
        sidechain: parent.is_some_and(|p| !p.is_empty()),
        ..Default::default()
    };
    let mut stmt = conn
        .prepare(
            "SELECT id,role,length(CAST(parts AS BLOB)),CAST(parts AS BLOB),model,created_at,
                    is_summary_message
             FROM messages WHERE session_id=?1 ORDER BY created_at,rowid",
        )
        .map_err(ReadError::Database)?;
    let mut rows = stmt.query([id]).map_err(ReadError::Database)?;
    while let Some(row) = rows.next().map_err(ReadError::Database)? {
        let key: String = row.get(0).map_err(ReadError::Database)?;
        let role: String = row.get(1).map_err(ReadError::Database)?;
        let size: u64 = row.get(2).map_err(ReadError::Database)?;
        if opts.max_line_bytes.is_some_and(|n| size > n as u64) {
            return Err(LineErrorKind::TooLong.into());
        }
        let bytes: Vec<u8> = row.get(3).map_err(ReadError::Database)?;
        let parts: Value =
            serde_json::from_slice(&bytes).map_err(|_| LineErrorKind::InvalidJson)?;
        let parts = parts
            .as_array()
            .ok_or_else(|| LineErrorKind::InvalidField("messages.parts".into()))?;
        let model: Option<String> = row.get(4).map_err(ReadError::Database)?;
        let created: Option<i64> = row.get(5).map_err(ReadError::Database)?;
        let summary: Option<i64> = row.get(6).map_err(ReadError::Database)?;
        let mut p = Parsed {
            include: opts.include,
            message_id: Some(key.clone()),
            at: adapters::seconds(created.map(Value::from).as_ref())?,
            ..Default::default()
        };
        message(&role, parts, summary == Some(1), model, &mut p)?;
        append(
            p,
            &state,
            vec![ImportSource::DatabaseRow {
                table: "messages".into(),
                key,
            }],
            result,
        );
        result.summary.lines += 1;
    }
    Ok(())
}

fn message(
    role: &str,
    parts: &[Value],
    summary: bool,
    model: Option<String>,
    p: &mut Parsed,
) -> Result<(), LineErrorKind> {
    let role = match role {
        "tool" => None,
        other => Some(
            Role::parse(other)
                .ok_or_else(|| LineErrorKind::InvalidField("messages.role".into()))?,
        ),
    };
    if model.is_some() {
        p.emit(
            0,
            Event::Meta(MetaUpdate {
                model,
                ..Default::default()
            }),
        );
    }
    let mut text = Vec::new();
    for (i, part) in parts.iter().enumerate() {
        let kind = crate::parser::required(part, "type")?;
        let data = part.get("data").unwrap_or(&Value::Null);
        match kind {
            "text" => text.push(Value::String(crate::parser::required(data, "text")?.into())),
            "tool_call" => adapters::call(
                crate::parser::string(data, "id"),
                crate::parser::required(data, "name")?,
                data.get("input"),
                3 + i,
                p,
            ),
            "tool_result" => adapters::result(
                crate::parser::string(data, "tool_call_id"),
                data.get("content").unwrap_or(&Value::Null),
                data.get("is_error").and_then(Value::as_bool),
                3 + i,
                p,
            )?,
            // Reasoning, media and run reports keep their native payload;
            // `finish` carries the stop reason and provider error text.
            "reasoning" | "image_url" | "binary" | "shell_command" | "sub_agent_report"
            | "finish" => adapters::native_content(role, kind, part, 3 + i, p),
            other => p.unknown.push(format!("crush:part:{other}")),
        }
    }
    match role {
        // A compaction summary replaces earlier context; it is not a new turn.
        Some(role) if summary => {
            adapters::native_content(Some(role), "summary", &Value::Array(text), 1, p)
        }
        Some(role) if !text.is_empty() => {
            let native = Value::Null;
            adapters::message(role, &Value::Array(text), &native, p)?;
        }
        Some(_) => {}
        None if text.is_empty() => {}
        None => p.unknown.push("crush:tool-row-text".into()),
    }
    Ok(())
}
