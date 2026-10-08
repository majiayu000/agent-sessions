//! Snapshot imports for native JSON, patch-based JSONL, and read-only databases.
//! JSON pointers / database row keys are retained instead of fabricated byte offsets.
mod cursor_cli;
mod database;
mod grok;
mod replay;
mod warp;
use crate::parser::{Parsed, State, tally};
use crate::*;
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use std::{
    fs::File,
    io::{BufRead, BufReader},
    path::Path,
};

// ProtoJSON cannot carry unknown wire fields. Preserve a diagnostic while
// returning known events, so a new field cannot look like a fully supported import.
pub(super) fn note_protobuf(message: &prost_reflect::DynamicMessage, summary: &mut ReadSummary) {
    fn visit(value: &prost_reflect::Value, summary: &mut ReadSummary) {
        match value {
            prost_reflect::Value::Message(m) => note_protobuf(m, summary),
            prost_reflect::Value::List(v) => {
                for item in v {
                    visit(item, summary);
                }
            }
            prost_reflect::Value::Map(v) => {
                for item in v.values() {
                    visit(item, summary);
                }
            }
            _ => {}
        }
    }
    for field in message.unknown_fields() {
        tally(
            &mut summary.unknown_types,
            &format!(
                "protobuf:{}:{}",
                prost_reflect::ReflectMessage::descriptor(message).full_name(),
                field.number()
            ),
        );
    }
    for (_, value) in message.fields() {
        visit(value, summary);
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum ImportSource {
    JsonPointer(String),
    Record(Location),
    DatabaseRow { table: String, key: String },
}
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ImportedEvent {
    /// All native records contributing to this event, including replay patches.
    pub sources: Vec<ImportSource>,
    pub at: Option<DateTime<Utc>>,
    pub timestamp_text: Option<String>,
    pub session_id: Option<String>,
    pub message_id: Option<String>,
    pub record_id: Option<String>,
    pub value: Event,
}
#[derive(Debug, Default, Serialize)]
pub struct SessionImport {
    pub events: Vec<ImportedEvent>,
    pub summary: ReadSummary,
}
#[derive(Debug)]
pub enum ImportError {
    Read(ReadError),
    Stream(StreamError),
}
impl std::fmt::Display for ImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Read(e) => e.fmt(f),
            Self::Stream(e) => e.fmt(f),
        }
    }
}
impl std::error::Error for ImportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(match self {
            Self::Read(e) => e,
            Self::Stream(e) => e,
        })
    }
}
impl From<ReadError> for ImportError {
    fn from(e: ReadError) -> Self {
        Self::Read(e)
    }
}
impl From<StreamError> for ImportError {
    fn from(e: StreamError) -> Self {
        Self::Stream(e)
    }
}
impl From<LineErrorKind> for ImportError {
    fn from(e: LineErrorKind) -> Self {
        Self::Read(ReadError::Format(e))
    }
}

/// Import an immutable file prefix. Database sources use import_database instead.
/// The result must pass summary.is_supported() before claiming complete support.
pub fn import_session(
    agent: Agent,
    path: &Path,
    opts: &ReadOptions,
) -> Result<SessionImport, ImportError> {
    let file = File::open(path).map_err(ReadError::Io)?;
    let mut opts = opts.clone();
    if opts.stop_at_byte.is_none() {
        opts.stop_at_byte = Some(file.metadata().map_err(ReadError::Io)?.len());
    }
    if let (Some(end), Some(limit)) = (opts.stop_at_byte, opts.max_file_bytes)
        && end > limit
    {
        return Err(ReadError::TooLarge { limit }.into());
    }
    import_session_from(agent, BufReader::new(file), &opts)
}
/// Reads one native session/export. Patch-based sources are replayed before events
/// are returned, so old text and rewound messages cannot masquerade as final text.
pub fn import_session_from<R: BufRead>(
    agent: Agent,
    source: R,
    opts: &ReadOptions,
) -> Result<SessionImport, ImportError> {
    opts.validate()?;
    if agent.streaming() && agent != Agent::KimiCli {
        let mut reader = read_from(agent, source, opts)?;
        let mut events = Vec::new();
        for e in reader.by_ref() {
            let e = e?;
            events.push(ImportedEvent {
                sources: vec![ImportSource::Record(e.location)],
                at: e.at,
                timestamp_text: e.timestamp_text,
                session_id: e.session_id,
                message_id: e.message_id,
                record_id: e.record_id,
                value: e.value,
            });
        }
        return Ok(SessionImport {
            events,
            summary: reader.finish(),
        });
    }
    if matches!(
        agent,
        Agent::Cursor | Agent::ZCode | Agent::CursorCli | Agent::Warp
    ) {
        return Err(ReadError::InvalidOptions(
            "this agent requires import_database with a native session ID",
        )
        .into());
    }
    // Reuse the existing bounded physical framer; no second IO/size contract.
    let mut raw = read_raw_from(
        source,
        &RawReadOptions {
            start_offset: 0,
            stop_at_byte: opts.stop_at_byte,
            max_read_bytes: opts.max_file_bytes,
            max_line_bytes: opts.max_line_bytes,
        },
    )?;
    let mut bytes = Vec::new();
    let mut records = Vec::new();
    for record in raw.by_ref() {
        let record = record?;
        bytes.extend_from_slice(&record.bytes);
        records.push(record);
    }
    let raw_summary = raw.finish();
    let summary = ReadSummary {
        status: raw_summary.status,
        lines: raw_summary.records,
        bytes_read: raw_summary.bytes_read,
        last_complete_byte: raw_summary.delivered_through,
        ..Default::default()
    };
    let mut result = SessionImport {
        summary,
        ..Default::default()
    };
    if agent == Agent::GeminiCli {
        // A legacy JSON document parses as one value; otherwise use native JSONL replay.
        if let Ok(v) = serde_json::from_slice::<Value>(&bytes)
            && v.get("messages").is_some()
        {
            document(agent, &v, opts, &mut result)?;
        } else {
            replay::gemini(&records, opts, &mut result)?;
        }
    } else if agent == Agent::KimiCli {
        replay::kimi(&records, opts, &mut result)?;
    } else if agent == Agent::Grok {
        grok::import(&records, opts, &mut result)?;
    } else {
        let v = serde_json::from_slice::<Value>(&bytes).map_err(|_| LineErrorKind::InvalidJson)?;
        document(agent, &v, opts, &mut result)?;
    }
    if result.summary.truncated_tail {
        result.summary.status = ReadStatus::IncompleteTail;
    }
    Ok(result)
}
fn array<'a>(v: &'a Value, key: &'static str) -> Result<&'a Vec<Value>, LineErrorKind> {
    v.get(key)
        .and_then(Value::as_array)
        .ok_or(LineErrorKind::MissingField(key))
}
pub(super) fn document(
    agent: Agent,
    v: &Value,
    opts: &ReadOptions,
    result: &mut SessionImport,
) -> Result<(), ImportError> {
    let mut state = State {
        codex_content: opts.codex_content,
        ..Default::default()
    };
    let (rows, prefix) = match agent {
        Agent::GeminiCli => {
            state.session_id = crate::parser::string(v, "sessionId");
            if v.get("kind").and_then(Value::as_str) == Some("subagent") {
                state.sidechain = true;
            }
            (array(v, "messages")?, "/messages")
        }
        Agent::Cline | Agent::RooCode => (
            v.as_array().ok_or_else(|| {
                LineErrorKind::InvalidField("API history must be an array".into())
            })?,
            "",
        ),
        Agent::ClineCli => {
            state.session_id = crate::parser::string(v, "sessionId");
            (array(v, "messages")?, "/messages")
        }
        Agent::Hermes => {
            state.session_id = crate::parser::string(v, "session_id");
            emit_meta(
                MetaUpdate {
                    session_id: state.session_id.clone(),
                    model: crate::parser::string(v, "model"),
                    ..Default::default()
                },
                vec![ImportSource::JsonPointer("".into())],
                opts,
                result,
            );
            (array(v, "messages")?, "/messages")
        }
        Agent::GrokBot => {
            let value = v.get("value").ok_or(LineErrorKind::MissingField("value"))?;
            (array(value, "entries")?, "/value/entries")
        }
        Agent::Zed => (array(v, "messages")?, "/messages"),
        Agent::Antigravity => {
            let trajectory = v.get("trajectory").unwrap_or(v);
            state.session_id = crate::parser::string(trajectory, "cascadeId")
                .or_else(|| crate::parser::string(trajectory, "trajectoryId"));
            let rows = array(trajectory, "steps")?;
            if let Some(total) = v.get("numTotalSteps") {
                let total = total
                    .as_u64()
                    .or_else(|| total.as_str().and_then(|v| v.parse().ok()))
                    .ok_or_else(|| LineErrorKind::InvalidField("numTotalSteps".into()))?;
                if total != rows.len() as u64 {
                    return Err(LineErrorKind::InvalidField(
                        "incomplete Antigravity trajectory; fetch all steps".into(),
                    )
                    .into());
                }
            }
            (
                rows,
                if v.get("trajectory").is_some() {
                    "/trajectory/steps"
                } else {
                    "/steps"
                },
            )
        }
        Agent::OpenCode => {
            let info = v.get("info").ok_or(LineErrorKind::MissingField("info"))?;
            state.session_id = crate::parser::string(info, "id");
            state.sidechain = info.get("parentID").is_some_and(|v| !v.is_null());
            emit_meta(
                MetaUpdate {
                    session_id: state.session_id.clone(),
                    cwd: crate::parser::string(info, "directory"),
                    ..Default::default()
                },
                vec![ImportSource::JsonPointer("/info".into())],
                opts,
                result,
            );
            (array(v, "messages")?, "/messages")
        }
        Agent::Goose => {
            state.session_id = crate::parser::string(v, "id");
            emit_meta(
                MetaUpdate {
                    session_id: state.session_id.clone(),
                    cwd: crate::parser::string(v, "working_dir"),
                    ..Default::default()
                },
                vec![ImportSource::JsonPointer("".into())],
                opts,
                result,
            );
            let c = v
                .get("conversation")
                .ok_or(LineErrorKind::MissingField("conversation"))?;
            if let Some(rows) = c.as_array() {
                (rows, "/conversation")
            } else {
                (array(c, "messages")?, "/conversation/messages")
            }
        }
        Agent::Continue => {
            state.session_id = crate::parser::string(v, "sessionId");
            emit_meta(
                MetaUpdate {
                    session_id: state.session_id.clone(),
                    cwd: crate::parser::string(v, "workspaceDirectory"),
                    model: crate::parser::string(v, "chatModelTitle"),
                    ..Default::default()
                },
                vec![ImportSource::JsonPointer("".into())],
                opts,
                result,
            );
            (array(v, "history")?, "/history")
        }
        _ => {
            return Err(
                ReadError::InvalidOptions("this agent does not use a JSON document").into(),
            );
        }
    };
    let mut tool_ids = std::collections::HashSet::new();
    let mut usage_ids = std::collections::HashSet::new();
    for (i, row) in rows.iter().enumerate() {
        let start = result.events.len();
        parse_record(
            agent,
            row,
            &mut state,
            vec![ImportSource::JsonPointer(format!("{prefix}/{i}"))],
            opts,
            result,
        )?;
        if agent == Agent::Antigravity {
            let mut tail = result.events.split_off(start);
            tail.retain(|e| match &e.value {
                Event::ToolCall(c) => c.id.as_ref().is_none_or(|id| tool_ids.insert(id.clone())),
                Event::Usage(u) => u
                    .dedup_key
                    .as_ref()
                    .is_none_or(|id| usage_ids.insert(id.clone())),
                _ => true,
            });
            result.events.extend(tail);
        }
    }
    if agent == Agent::Zed
        && opts.include.contains(EventKinds::USAGE)
        && let Some(usage) = v.get("request_token_usage").and_then(Value::as_object)
    {
        for (id, counts) in usage {
            let mut p = Parsed {
                include: opts.include,
                ..Default::default()
            };
            p.emit(
                0,
                Event::Usage(Usage {
                    counts: TokenCounts {
                        input: crate::adapters::counter(counts, "input_tokens")?,
                        output: crate::adapters::counter(counts, "output_tokens")?,
                        cache_read: crate::adapters::counter(counts, "cache_read_input_tokens")?,
                        cache_write: crate::adapters::counter(
                            counts,
                            "cache_creation_input_tokens",
                        )?,
                        ..Default::default()
                    },
                    semantics: TokenSemantics::Unknown,
                    model: None,
                    dedup_key: Some(id.clone()),
                    cumulative: None,
                    basis: UsageBasis::Response,
                    stop_reason: None,
                    endpoint: Endpoint::Unknown,
                    inference_geo: None,
                    adjustments: Vec::new(),
                }),
            );
            append(
                p,
                &state,
                vec![ImportSource::JsonPointer(format!(
                    "/request_token_usage/{}",
                    id.replace('~', "~0").replace('/', "~1")
                ))],
                result,
            );
        }
    }
    Ok(())
}
pub(crate) fn parse_record(
    agent: Agent,
    v: &Value,
    state: &mut State,
    sources: Vec<ImportSource>,
    opts: &ReadOptions,
    result: &mut SessionImport,
) -> Result<(), ImportError> {
    let mut p = Parsed {
        include: opts.include,
        accounting: opts.accounting,
        ..Default::default()
    };
    p.at = if matches!(agent, Agent::GeminiCli | Agent::QwenCode) {
        crate::parser::timestamp(v)?
    } else {
        None
    };
    p.timestamp_text = v
        .get("timestamp")
        .and_then(Value::as_str)
        .map(str::to_owned);
    if matches!(agent, Agent::Cline | Agent::RooCode | Agent::ClineCli) {
        p.at = crate::adapters::millis(v.get("ts"))?;
    }
    if agent == Agent::Hermes {
        p.at = crate::adapters::seconds(v.get("timestamp"))?;
    }
    crate::adapters::parse(agent, v, state, &mut p)?;
    append(p, state, sources, result);
    Ok(())
}
pub(crate) fn append(
    p: Parsed,
    state: &State,
    sources: Vec<ImportSource>,
    result: &mut SessionImport,
) {
    for unknown in p.unknown {
        tally(&mut result.summary.unknown_types, &unknown);
    }
    for ignored in p.ignored {
        tally(&mut result.summary.ignored_types, &ignored);
    }
    for (_, mut value) in p.events {
        if let Event::Message(m) = &mut value {
            m.is_sidechain |= state.sidechain;
        }
        result.events.push(ImportedEvent {
            sources: sources.clone(),
            at: p.at,
            timestamp_text: p.timestamp_text.clone(),
            session_id: state.session_id.clone(),
            message_id: p.message_id.clone(),
            record_id: p.record_id.clone(),
            value,
        });
    }
}
pub(crate) fn emit_meta(
    meta: MetaUpdate,
    sources: Vec<ImportSource>,
    opts: &ReadOptions,
    result: &mut SessionImport,
) {
    if opts.include.contains(EventKinds::META) {
        result.events.push(ImportedEvent {
            session_id: meta.session_id.clone(),
            sources,
            at: None,
            timestamp_text: None,
            message_id: None,
            record_id: None,
            value: Event::Meta(meta),
        });
    }
}
pub(crate) fn decode_record(
    record: &RawRecord,
    opts: &ReadOptions,
    result: &mut SessionImport,
) -> Result<Option<Value>, ImportError> {
    if record.bytes.iter().all(u8::is_ascii_whitespace) {
        return Ok(None);
    }
    if std::str::from_utf8(&record.bytes).is_err() {
        return Err(StreamError::Line {
            line_no: record.line_no,
            byte_start: record.byte_start,
            kind: LineErrorKind::InvalidUtf8,
        }
        .into());
    }
    match serde_json::from_slice(&record.bytes) {
        Ok(v) => Ok(Some(v)),
        Err(e) if !record.terminated && e.is_eof() && opts.tail == TailMode::AllowIncomplete => {
            result.summary.truncated_tail = true;
            result.summary.last_complete_byte = record.byte_start;
            Ok(None)
        }
        Err(_) => Err(StreamError::Line {
            line_no: record.line_no,
            byte_start: record.byte_start,
            kind: LineErrorKind::InvalidJson,
        }
        .into()),
    }
}
pub(crate) fn record_source(r: &RawRecord) -> ImportSource {
    ImportSource::Record(Location {
        record_index: r.line_no - 1,
        line_no: r.line_no,
        byte_start: r.byte_start,
        byte_end: r.byte_end,
        event_index: 0,
    })
}
pub use database::{DatabaseSession, import_database, list_database_sessions};
