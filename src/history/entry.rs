use crate::{Agent, LineErrorKind};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub session_id: Option<String>,
    pub text: Option<String>,
    pub at: Option<DateTime<Utc>>,
    pub project: Option<String>,
    pub invalid_fields: Vec<String>,
}
pub fn history_entry(agent: Agent, v: &Value, strict: bool) -> Result<HistoryEntry, LineErrorKind> {
    let mut invalid_fields: Vec<String> = Vec::new();
    if !v.is_object() {
        invalid_fields.push("record".into());
    }
    let (id_key, text_key, time_key) = match agent {
        Agent::ClaudeCode => ("sessionId", "display", "timestamp"),
        Agent::Codex => ("session_id", "text", "ts"),
    };
    let mut field = |key: &str| match v.get(key) {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(s.clone()),
        _ => {
            invalid_fields.push(key.into());
            None
        }
    };
    let session_id = field(id_key);
    let text = field(text_key);
    let project = field("project");
    let at = match v.get(time_key) {
        None | Some(Value::Null) => None,
        Some(t) => {
            let at = t.as_i64().and_then(|n| match agent {
                Agent::ClaudeCode => DateTime::from_timestamp_millis(n),
                Agent::Codex => DateTime::from_timestamp(n, 0),
            });
            if at.is_none() {
                invalid_fields.push(time_key.into());
            }
            at
        }
    };
    if strict && let Some(key) = invalid_fields.first() {
        return Err(LineErrorKind::InvalidField(key.clone()));
    }
    Ok(HistoryEntry {
        session_id,
        text,
        at,
        project,
        invalid_fields,
    })
}
