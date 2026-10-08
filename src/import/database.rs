use super::*;
use rusqlite::{Connection, OpenFlags};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DatabaseSession {
    pub id: String,
    pub title: Option<String>,
    pub cwd: Option<String>,
}
fn open(agent: Agent, path: &Path, opts: &ReadOptions) -> Result<Connection, ImportError> {
    if !matches!(
        agent,
        Agent::OpenCode
            | Agent::Cursor
            | Agent::Goose
            | Agent::Hermes
            | Agent::ZCode
            | Agent::CursorCli
            | Agent::Zed
            | Agent::Warp
    ) {
        return Err(ReadError::InvalidOptions("no database adapter for this agent").into());
    }
    if opts.stop_at_byte.is_some() {
        return Err(ReadError::InvalidOptions(
            "database snapshots do not have byte-prefix boundaries",
        )
        .into());
    }
    let size = std::fs::metadata(path).map_err(ReadError::Io)?.len();
    if let Some(limit) = opts.max_file_bytes
        && size > limit
    {
        return Err(ReadError::TooLarge { limit }.into());
    }
    Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| ReadError::Database(e).into())
}
/// Enumerate native IDs; never creates or migrates the database.
pub fn list_database_sessions(
    agent: Agent,
    path: &Path,
    opts: &ReadOptions,
) -> Result<Vec<DatabaseSession>, ImportError> {
    let conn = open(agent, path, opts)?;
    let mut result = Vec::new();
    if matches!(agent, Agent::OpenCode | Agent::ZCode) {
        let mut statement = conn
            .prepare("SELECT id,title,directory FROM session ORDER BY time_created,id")
            .map_err(ReadError::Database)?;
        let rows = statement
            .query_map([], |r| {
                Ok(DatabaseSession {
                    id: r.get(0)?,
                    title: r.get(1)?,
                    cwd: r.get(2)?,
                })
            })
            .map_err(ReadError::Database)?;
        for row in rows {
            result.push(row.map_err(ReadError::Database)?);
        }
    } else if agent == Agent::Goose {
        let mut statement = conn
            .prepare("SELECT id,name,working_dir FROM sessions ORDER BY created_at,id")
            .map_err(ReadError::Database)?;
        let rows = statement
            .query_map([], |r| {
                Ok(DatabaseSession {
                    id: r.get(0)?,
                    title: r.get(1)?,
                    cwd: r.get(2)?,
                })
            })
            .map_err(ReadError::Database)?;
        for row in rows {
            result.push(row.map_err(ReadError::Database)?);
        }
    } else if agent == Agent::Hermes {
        let mut statement = conn
            .prepare("SELECT id,title,cwd FROM sessions ORDER BY started_at,id")
            .map_err(ReadError::Database)?;
        for row in statement
            .query_map([], |r| {
                Ok(DatabaseSession {
                    id: r.get(0)?,
                    title: r.get(1)?,
                    cwd: r.get(2)?,
                })
            })
            .map_err(ReadError::Database)?
        {
            result.push(row.map_err(ReadError::Database)?);
        }
    } else if agent == Agent::Warp {
        let mut statement=conn.prepare("SELECT conversation_id,summary FROM agent_conversations ORDER BY last_modified_at,id").map_err(ReadError::Database)?;
        for row in statement
            .query_map([], |r| {
                Ok(DatabaseSession {
                    id: r.get(0)?,
                    title: r.get(1)?,
                    cwd: None,
                })
            })
            .map_err(ReadError::Database)?
        {
            result.push(row.map_err(ReadError::Database)?);
        }
    } else if agent == Agent::CursorCli {
        let meta = super::cursor_cli::metadata(&conn, opts)?;
        result.push(DatabaseSession {
            id: crate::parser::required(&meta, "agentId")?.into(),
            title: crate::parser::string(&meta, "name"),
            cwd: None,
        });
    } else if agent == Agent::Zed {
        let mut statement = conn
            .prepare("SELECT id,summary FROM threads ORDER BY updated_at,id")
            .map_err(ReadError::Database)?;
        for row in statement
            .query_map([], |r| {
                Ok(DatabaseSession {
                    id: r.get(0)?,
                    title: r.get(1)?,
                    cwd: None,
                })
            })
            .map_err(ReadError::Database)?
        {
            result.push(row.map_err(ReadError::Database)?);
        }
    } else {
        let mut statement = conn
            .prepare("SELECT key FROM cursorDiskKV WHERE key LIKE 'composerData:%' ORDER BY key")
            .map_err(ReadError::Database)?;
        let rows = statement
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(ReadError::Database)?;
        for row in rows {
            let key = row.map_err(ReadError::Database)?;
            result.push(DatabaseSession {
                id: key["composerData:".len()..].into(),
                title: None,
                cwd: None,
            });
        }
    }
    Ok(result)
}
fn json(conn: &Connection, sql: &str, key: &str, opts: &ReadOptions) -> Result<Value, ImportError> {
    // Check serialized row size in SQL before copying the value into Rust.
    let mut stmt = conn.prepare(sql).map_err(ReadError::Database)?;
    let mut rows = stmt.query([key]).map_err(ReadError::Database)?;
    let row = rows
        .next()
        .map_err(ReadError::Database)?
        .ok_or(LineErrorKind::MissingField("database record"))?;
    let size = row.get::<_, u64>(0).map_err(ReadError::Database)?;
    if opts.max_line_bytes.is_some_and(|limit| size > limit as u64) {
        return Err(LineErrorKind::TooLong.into());
    }
    let bytes = row.get::<_, Vec<u8>>(1).map_err(ReadError::Database)?;
    serde_json::from_slice(&bytes).map_err(|_| LineErrorKind::InvalidJson.into())
}
fn source(table: &str, key: &str) -> ImportSource {
    ImportSource::DatabaseRow {
        table: table.into(),
        key: key.into(),
    }
}
/// Read one session within a SQLite read transaction. No byte offsets are invented.
/// opts.max_line_bytes bounds each serialized row; max_file_bytes bounds the DB file.
pub fn import_database(
    agent: Agent,
    path: &Path,
    id: &str,
    opts: &ReadOptions,
) -> Result<SessionImport, ImportError> {
    let mut conn = open(agent, path, opts)?;
    let tx = conn.transaction().map_err(ReadError::Database)?;
    let mut result = SessionImport::default();
    let mut state = State {
        session_id: Some(id.into()),
        ..Default::default()
    };
    if agent == Agent::Warp {
        super::warp::import(&tx, id, opts, &mut result)?;
    } else if agent == Agent::CursorCli {
        super::cursor_cli::import(&tx, id, opts, &mut result)?;
    } else if agent == Agent::Zed {
        let (data_type, parent, length): (String, Option<String>, u64) = tx
            .query_row(
                "SELECT data_type,parent_id,length(data) FROM threads WHERE id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .map_err(ReadError::Database)?;
        if opts.max_line_bytes.is_some_and(|n| length > n as u64) {
            return Err(LineErrorKind::TooLong.into());
        }
        let data: Vec<u8> = tx
            .query_row("SELECT data FROM threads WHERE id=?1", [id], |r| r.get(0))
            .map_err(ReadError::Database)?;
        let bytes = match data_type.as_str() {
            "json" => data,
            "zstd" => {
                let decoder =
                    zstd::stream::read::Decoder::new(data.as_slice()).map_err(ReadError::Io)?;
                let limit = opts.max_line_bytes.map(|n| n as u64).unwrap_or(u64::MAX);
                let mut decoded = Vec::new();
                std::io::Read::read_to_end(
                    &mut std::io::Read::take(decoder, limit.saturating_add(1)),
                    &mut decoded,
                )
                .map_err(ReadError::Io)?;
                if decoded.len() as u64 > limit {
                    return Err(LineErrorKind::TooLong.into());
                }
                decoded
            }
            _ => return Err(LineErrorKind::InvalidField("threads.data_type".into()).into()),
        };
        let value: Value =
            serde_json::from_slice(&bytes).map_err(|_| LineErrorKind::InvalidJson)?;
        super::document(agent, &value, opts, &mut result)?;
        for event in &mut result.events {
            event.sources = vec![source("threads", id)];
            event.session_id = Some(id.into());
            if let Event::Message(m) = &mut event.value {
                m.is_sidechain |= parent.is_some();
            }
        }
    } else if matches!(agent, Agent::OpenCode | Agent::ZCode) {
        let (directory, parent): (String, Option<String>) = tx
            .query_row(
                "SELECT directory,parent_id FROM session WHERE id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(ReadError::Database)?;
        state.sidechain = parent.is_some();
        emit_meta(
            MetaUpdate {
                session_id: Some(id.into()),
                cwd: Some(directory),
                ..Default::default()
            },
            vec![source("session", id)],
            opts,
            &mut result,
        );
        let ids = {
            let mut stmt = tx
                .prepare(if agent == Agent::ZCode {
                    "SELECT id FROM message WHERE session_id=?1 ORDER BY sequence,id"
                } else {
                    "SELECT id FROM message WHERE session_id=?1 ORDER BY time_created,id"
                })
                .map_err(ReadError::Database)?;
            stmt.query_map([id], |r| r.get::<_, String>(0))
                .map_err(ReadError::Database)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(ReadError::Database)?
        };
        for message_id in ids {
            let mut info = json(
                &tx,
                "SELECT length(CAST(data AS BLOB)),CAST(data AS BLOB) FROM message WHERE id=?1",
                &message_id,
                opts,
            )?;
            if !info.is_object() {
                return Err(LineErrorKind::InvalidField("message.data".into()).into());
            }
            info["id"] = Value::String(message_id.clone());
            info["sessionID"] = Value::String(id.into());
            let ids = {
                let mut stmt=tx.prepare(if agent == Agent::ZCode { "SELECT id FROM part WHERE message_id=?1 AND session_id=?2 ORDER BY sequence,id" } else { "SELECT id FROM part WHERE message_id=?1 AND session_id=?2 ORDER BY time_created,id" }).map_err(ReadError::Database)?;
                stmt.query_map([&message_id, id], |r| r.get::<_, String>(0))
                    .map_err(ReadError::Database)?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(ReadError::Database)?
            };
            let mut parts = Vec::new();
            let mut sources = vec![source("message", &message_id)];
            for part_id in ids {
                parts.push(json(
                    &tx,
                    "SELECT length(CAST(data AS BLOB)),CAST(data AS BLOB) FROM part WHERE id=?1",
                    &part_id,
                    opts,
                )?);
                sources.push(source("part", &part_id));
            }
            let record = serde_json::json!({"info":info,"parts":parts});
            parse_record(agent, &record, &mut state, sources, opts, &mut result)?;
            result.summary.lines += 1;
        }
    } else if agent == Agent::Goose {
        let directory: String = tx
            .query_row("SELECT working_dir FROM sessions WHERE id=?1", [id], |r| {
                r.get(0)
            })
            .map_err(ReadError::Database)?;
        emit_meta(
            MetaUpdate {
                session_id: Some(id.into()),
                cwd: Some(directory),
                ..Default::default()
            },
            vec![source("sessions", id)],
            opts,
            &mut result,
        );
        let mut stmt=tx.prepare("SELECT id,message_id,role,created_timestamp,length(CAST(content_json AS BLOB)),CAST(content_json AS BLOB),length(CAST(metadata_json AS BLOB)),CAST(metadata_json AS BLOB) FROM messages WHERE session_id=?1 ORDER BY created_timestamp,id").map_err(ReadError::Database)?;
        let mut rows = stmt.query([id]).map_err(ReadError::Database)?;
        while let Some(row) = rows.next().map_err(ReadError::Database)? {
            let content_size: u64 = row.get(4).map_err(ReadError::Database)?;
            let metadata_size: Option<u64> = row.get(6).map_err(ReadError::Database)?;
            if opts.max_line_bytes.is_some_and(|limit| {
                content_size.saturating_add(metadata_size.unwrap_or(0)) > limit as u64
            }) {
                return Err(LineErrorKind::TooLong.into());
            }
            let content: Vec<u8> = row.get(5).map_err(ReadError::Database)?;
            let metadata: Option<Vec<u8>> = row.get(7).map_err(ReadError::Database)?;
            let content: Value =
                serde_json::from_slice(&content).map_err(|_| LineErrorKind::InvalidJson)?;
            let metadata: Value = metadata
                .map(|v| serde_json::from_slice(&v))
                .transpose()
                .map_err(|_| LineErrorKind::InvalidJson)?
                .unwrap_or(Value::Null);
            let row_id: i64 = row.get(0).map_err(ReadError::Database)?;
            let message_id: Option<String> = row.get(1).map_err(ReadError::Database)?;
            let role: String = row.get(2).map_err(ReadError::Database)?;
            let created: i64 = row.get(3).map_err(ReadError::Database)?;
            let record = serde_json::json!({"id":message_id,"role":role,"created":created,"content":content,"metadata":metadata});
            parse_record(
                agent,
                &record,
                &mut state,
                vec![source("messages", &row_id.to_string())],
                opts,
                &mut result,
            )?;
            result.summary.lines += 1;
        }
    } else if agent == Agent::Hermes {
        let (model, cwd, parent): (Option<String>, Option<String>, Option<String>) = tx
            .query_row(
                "SELECT model,cwd,parent_session_id FROM sessions WHERE id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .map_err(ReadError::Database)?;
        state.sidechain = parent.is_some();
        emit_meta(
            MetaUpdate {
                session_id: Some(id.into()),
                model,
                cwd,
                ..Default::default()
            },
            vec![source("sessions", id)],
            opts,
            &mut result,
        );
        let mut stmt = tx.prepare("SELECT id,role,length(CAST(content AS BLOB)),content,tool_call_id,length(CAST(tool_calls AS BLOB)),tool_calls,timestamp FROM messages WHERE session_id=?1 ORDER BY timestamp,id").map_err(ReadError::Database)?;
        let mut rows = stmt.query([id]).map_err(ReadError::Database)?;
        while let Some(row) = rows.next().map_err(ReadError::Database)? {
            let content_size: Option<u64> = row.get(2).map_err(ReadError::Database)?;
            let calls_size: Option<u64> = row.get(5).map_err(ReadError::Database)?;
            if opts.max_line_bytes.is_some_and(|limit| {
                content_size
                    .unwrap_or(0)
                    .saturating_add(calls_size.unwrap_or(0))
                    > limit as u64
            }) {
                return Err(LineErrorKind::TooLong.into());
            }
            let row_id: i64 = row.get(0).map_err(ReadError::Database)?;
            let role: String = row.get(1).map_err(ReadError::Database)?;
            let content: Option<String> = row.get(3).map_err(ReadError::Database)?;
            // Hermes explicitly tags structured content. Plain JSON-looking text stays text.
            let content = match content {
                Some(s) if s.starts_with("\0json:") => {
                    serde_json::from_str(&s[6..]).map_err(|_| LineErrorKind::InvalidJson)?
                }
                Some(s) => Value::String(s),
                None => Value::Null,
            };
            let call_id: Option<String> = row.get(4).map_err(ReadError::Database)?;
            let calls: Option<String> = row.get(6).map_err(ReadError::Database)?;
            let calls: Value = calls
                .map(|s| serde_json::from_str(&s))
                .transpose()
                .map_err(|_| LineErrorKind::InvalidJson)?
                .unwrap_or(Value::Null);
            let timestamp: Option<f64> = row.get(7).map_err(ReadError::Database)?;
            let mut record = serde_json::json!({"role":role,"content":content,"tool_call_id":call_id,"timestamp":timestamp});
            if !calls.is_null() {
                record["tool_calls"] = calls;
            }
            parse_record(
                agent,
                &record,
                &mut state,
                vec![source("messages", &row_id.to_string())],
                opts,
                &mut result,
            )?;
            result.summary.lines += 1;
        }
        tally(
            &mut result.summary.ignored_types,
            "hermes:session-totals-not-message-usage",
        );
    } else {
        let key = format!("composerData:{id}");
        let composer = json(
            &tx,
            "SELECT length(CAST(value AS BLOB)),CAST(value AS BLOB) FROM cursorDiskKV WHERE key=?1",
            &key,
            opts,
        )?;
        emit_meta(
            MetaUpdate {
                session_id: Some(id.into()),
                model: composer
                    .pointer("/modelConfig/modelName")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                ..Default::default()
            },
            vec![source("cursorDiskKV", &key)],
            opts,
            &mut result,
        );
        let headers = composer
            .get("fullConversationHeadersOnly")
            .and_then(Value::as_array)
            .ok_or(LineErrorKind::MissingField("fullConversationHeadersOnly"))?;
        for header in headers {
            let bubble = crate::parser::required(header, "bubbleId")?;
            let key = format!("bubbleId:{id}:{bubble}");
            let record = json(
                &tx,
                "SELECT length(CAST(value AS BLOB)),CAST(value AS BLOB) FROM cursorDiskKV WHERE key=?1",
                &key,
                opts,
            )?;
            parse_record(
                agent,
                &record,
                &mut state,
                vec![source("cursorDiskKV", &key)],
                opts,
                &mut result,
            )?;
            result.summary.lines += 1;
        }
    }
    // Dropping a read transaction rolls back only its read snapshot; no DB writes.
    result.summary.status = ReadStatus::Complete;
    Ok(result)
}
