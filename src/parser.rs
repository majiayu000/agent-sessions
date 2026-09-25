use crate::{Agent, CodexUsageMode, Event, EventKinds, LineErrorKind, TokenCounts};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default)]
pub(crate) struct State {
    pub model: Option<String>,
    pub session_id: Option<String>,
    pub previous: Option<TokenCounts>,
    pub broken_usage: bool,
    pub sidechain: bool,
}
#[derive(Default)]
pub(crate) struct Parsed {
    pub events: Vec<(usize, Event)>,
    pub include: EventKinds,
    pub unknown: Vec<String>,
    pub ignored: Vec<String>,
    pub at: Option<DateTime<Utc>>,
    pub message_id: Option<String>,
}
impl Parsed {
    pub fn wants(&self, kind: EventKinds) -> bool {
        self.include.contains(kind)
    }
    pub fn emit(&mut self, slot: usize, event: Event) {
        if self.include.includes(&event) {
            self.events.push((slot, event));
        }
    }
}
pub(crate) fn flag(v: Option<&Value>, key: &'static str) -> Result<bool, LineErrorKind> {
    match v {
        None | Some(Value::Null) => Ok(false),
        Some(Value::Bool(b)) => Ok(*b),
        _ => Err(LineErrorKind::InvalidField(key.into())),
    }
}
pub(crate) fn string(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(str::to_owned)
}
pub(crate) fn required<'a>(v: &'a Value, key: &'static str) -> Result<&'a str, LineErrorKind> {
    v.get(key)
        .and_then(Value::as_str)
        .ok_or(LineErrorKind::MissingField(key))
}
pub(crate) fn timestamp(v: &Value) -> Result<Option<DateTime<Utc>>, LineErrorKind> {
    let t = v
        .get("timestamp")
        .or_else(|| v.get("created_at"))
        .or_else(|| v.get("createdAt"))
        .or_else(|| v.pointer("/payload/timestamp"))
        .or_else(|| v.pointer("/data/message/timestamp"));
    let Some(t) = t.filter(|t| !t.is_null()) else {
        return Ok(None);
    };
    let date = t
        .as_i64()
        .or_else(|| t.as_str()?.trim().parse().ok())
        .and_then(|s| DateTime::from_timestamp(s, 0))
        .or_else(|| {
            DateTime::parse_from_rfc3339(t.as_str()?.trim())
                .ok()
                .map(|d| d.with_timezone(&Utc))
        });
    date.map(Some)
        .ok_or(LineErrorKind::InvalidField("timestamp".into()))
}
pub(crate) fn text(v: &Value, parsed: &mut Parsed) -> Result<String, LineErrorKind> {
    text_projection(v, parsed).map(|(text, _)| text)
}
pub(crate) fn text_projection(
    v: &Value,
    parsed: &mut Parsed,
) -> Result<(String, Vec<std::ops::Range<usize>>), LineErrorKind> {
    if let Some(s) = v.as_str() {
        return Ok((s.to_owned(), std::iter::once(0..s.len()).collect()));
    }
    let blocks = v
        .as_array()
        .ok_or(LineErrorKind::InvalidField("content".into()))?;
    let mut parts = Vec::new();
    for b in blocks {
        match required(b, "type")? {
            "text" | "input_text" | "output_text" => parts.push(required(b, "text")?.to_owned()),
            "tool_use" | "tool_result" | "server_tool_use" | "thinking" | "redacted_thinking"
            | "image" | "input_image" | "output_image" | "document" => {}
            "tool_reference" => parsed.ignored.push("content:tool_reference".into()),
            other => parsed.unknown.push(format!("content:{other}")),
        }
    }
    let mut segments = Vec::new();
    let mut offset = 0;
    for part in &parts {
        segments.push(offset..offset + part.len());
        offset += part.len() + 1;
    }
    Ok((parts.join("\n"), segments))
}
pub(crate) fn parse(
    agent: Agent,
    v: &Value,
    state: &mut State,
    mode: CodexUsageMode,
    include: EventKinds,
) -> Result<Parsed, LineErrorKind> {
    let mut candidate = state.clone();
    let mut parsed = Parsed {
        at: timestamp(v)?,
        include,
        ..Parsed::default()
    };
    let result = match agent {
        Agent::ClaudeCode => crate::claude::parse(v, &mut candidate, &mut parsed),
        Agent::Codex => crate::codex::parse(v, &mut candidate, &mut parsed, mode),
    };
    if let Err(kind) = result {
        if matches!(
            kind,
            LineErrorKind::CounterRegression | LineErrorKind::LostUsageBaseline
        ) {
            // Valid cumulative sample restores the baseline but emits no guessed usage.
            *state = candidate;
        }
        return Err(kind);
    }
    *state = candidate;
    parsed.events.sort_by_key(|(slot, _)| *slot);
    Ok(parsed)
}
pub(crate) fn tally(map: &mut BTreeMap<String, u64>, key: &str) {
    // Avoid retaining arbitrarily many or arbitrarily long attacker-controlled labels.
    let key: String = key.chars().take(96).collect();
    let key = if map.len() >= 128 && !map.contains_key(&key) {
        "<other>".into()
    } else {
        key
    };
    let count = map.entry(key).or_default();
    *count = count.saturating_add(1);
}
