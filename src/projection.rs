//! Tolerant projections for consumers that retain the original JSON record.
//! These helpers do not validate records or establish snapshot completeness.
use crate::{Agent, Message, MetaUpdate, Role};
use chrono::{DateTime, Utc};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversationMessage {
    pub role: Role,
    pub text: String,
    pub created_at_epoch: Option<i64>,
}

/// First present native timestamp field, in seconds. Invalid values stay absent;
/// a malformed earlier field does not fall through to a later field.
pub fn tolerant_timestamp_epoch(v: &Value) -> Option<i64> {
    let t = v
        .get("timestamp")
        .or_else(|| v.get("created_at"))
        .or_else(|| v.get("createdAt"))
        .or_else(|| v.pointer("/payload/timestamp"))?;
    t.as_i64()
        .or_else(|| t.as_str()?.trim().parse().ok())
        .or_else(|| {
            DateTime::parse_from_rfc3339(t.as_str()?.trim())
                .ok()
                .map(|d| d.timestamp())
        })
}

fn text(
    content: Option<&Value>,
    allowed: &[&str],
    strings: bool,
) -> (String, Vec<std::ops::Range<usize>>) {
    let Some(content) = content else {
        return (String::new(), Vec::new());
    };
    let parts: Vec<&str> = if strings && content.is_string() {
        content.as_str().into_iter().collect()
    } else {
        content
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|block| {
                allowed
                    .contains(&block.get("type")?.as_str()?)
                    .then(|| block.get("text")?.as_str())
                    .flatten()
            })
            .collect()
    };
    let mut offset = 0;
    let segments = parts
        .iter()
        .map(|s| {
            let range = offset..offset + s.len();
            offset += s.len() + 1;
            range
        })
        .collect();
    (parts.join("\n"), segments)
}

/// Project user/assistant records without filtering meta/control content. Empty
/// or malformed content remains an empty message for archival classification.
pub fn project_conversation(v: &Value) -> Option<ConversationMessage> {
    let (role, content) = match v.get("type")?.as_str()? {
        "user" => (Role::User, v.pointer("/message/content")),
        "assistant" => (Role::Assistant, v.pointer("/message/content")),
        "response_item" if v.pointer("/payload/type")?.as_str()? == "message" => {
            let role = match v.pointer("/payload/role")?.as_str()? {
                "user" => Role::User,
                "assistant" => Role::Assistant,
                _ => return None,
            };
            (role, v.pointer("/payload/content"))
        }
        _ => return None,
    };
    Some(ConversationMessage {
        role,
        text: text(content, &["text", "input_text", "output_text"], true).0,
        created_at_epoch: tolerant_timestamp_epoch(v),
    })
}

/// Native fields are borrowed without coercion; callers decide whether arguments
/// and outputs must be strings, objects, or other JSON types.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CodexFunction<'a> {
    Call {
        call_id: Option<&'a str>,
        name: Option<&'a str>,
        arguments: Option<&'a Value>,
    },
    Output {
        call_id: Option<&'a str>,
        output: Option<&'a Value>,
    },
}
pub fn project_codex_function(v: &Value) -> Option<CodexFunction<'_>> {
    if v.get("type")?.as_str()? != "response_item" {
        return None;
    }
    let p = v.get("payload")?;
    let call_id = p.get("call_id").and_then(Value::as_str);
    match p.get("type")?.as_str()? {
        "function_call" => Some(CodexFunction::Call {
            call_id,
            name: p.get("name").and_then(Value::as_str),
            arguments: p.get("arguments"),
        }),
        "function_call_output" => Some(CodexFunction::Output {
            call_id,
            output: p.get("output"),
        }),
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TranscriptProjection {
    pub message: Option<Message>,
    pub meta: MetaUpdate,
    pub at: Option<DateTime<Utc>>,
}

/// Tolerant native transcript projection, including legacy Codex envelopes.
/// Claude accepts text blocks; current Codex accepts role-specific text blocks.
/// Unknown fields do not invalidate valid text. JSON syntax and tail policy are
/// the caller's responsibility. Meta messages are tagged, not filtered.
pub fn project_transcript(agent: Agent, v: &Value) -> TranscriptProjection {
    let at = [v.get("timestamp"), v.pointer("/payload/timestamp")]
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .find_map(|s| {
            DateTime::parse_from_rfc3339(s)
                .ok()
                .map(|d| d.with_timezone(&Utc))
        });
    let kind = v.get("type").and_then(Value::as_str).unwrap_or("");
    let mut meta = MetaUpdate::default();
    let mut candidate = None;
    match agent {
        Agent::ClaudeCode => {
            meta.cwd = crate::parser::string(v, "cwd");
            if kind == "system" {
                meta.model = v
                    .pointer("/message/model")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
            }
            if matches!(kind, "user" | "assistant") {
                candidate = Some((
                    if kind == "user" {
                        Role::User
                    } else {
                        Role::Assistant
                    },
                    text(v.pointer("/message/content"), &["text"], true),
                ));
            }
        }
        Agent::Codex => match kind {
            "session_meta" | "turn_context" => {
                let p = v.get("payload").unwrap_or(&Value::Null);
                meta.model = p
                    .get("model")
                    .or_else(|| (kind == "session_meta").then(|| v.get("model")).flatten())
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                meta.cwd = crate::parser::string(p, "cwd");
                meta.origin = Some(crate::codex::origin::classify(p));
                meta.source = p.get("source").cloned();
                meta.originator = crate::parser::string(p, "originator");
                meta.thread_source = crate::parser::string(p, "thread_source");
            }
            "user_message" => {
                if v.get("content").is_some_and(Value::is_string) {
                    candidate = Some((Role::User, text(v.get("content"), &[], true)));
                }
            }
            "response_item" => {
                let p = v.get("payload").unwrap_or(&Value::Null);
                let ty = p.get("type").and_then(Value::as_str);
                let role = p.get("role").and_then(Value::as_str);
                let selection: Option<(Role, &[&str])> = match (ty, role) {
                    (Some("message"), Some("user")) => Some((Role::User, &["input_text", "text"])),
                    (Some("message"), Some("assistant")) => {
                        Some((Role::Assistant, &["output_text", "text"]))
                    }
                    (None, _) => Some((Role::Assistant, &["text"])),
                    _ => None,
                };
                if let Some((role, allowed)) = selection {
                    candidate = Some((role, text(p.get("content"), allowed, false)));
                }
            }
            _ => {}
        },
    }
    let message =
        candidate
            .filter(|(_, (body, _))| !body.is_empty())
            .map(|(role, (text, text_segments))| Message {
                role,
                text,
                text_segments,
                is_meta: v
                    .get("isMeta")
                    .or_else(|| v.get("is_meta"))
                    .or_else(|| v.pointer("/message/isMeta"))
                    .or_else(|| v.pointer("/message/is_meta"))
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                is_sidechain: v
                    .get("isSidechain")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                parent_id: crate::parser::string(v, "parentUuid"),
            });
    TranscriptProjection { message, meta, at }
}
