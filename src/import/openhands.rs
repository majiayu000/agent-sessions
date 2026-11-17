//! OpenHands CLI conversations (`~/.openhands/conversations/<id>/`, SDK 1.2x).
//!
//! Each event is its own `events/event-<idx>-<event id>.json`, discriminated by
//! `kind`; `base_state.json` holds the conversation state, including a per-call
//! token ledger in `stats.usage_to_metrics.*.token_usages`. Event timestamps are
//! local times without an offset, so only the native text is kept. Condensation
//! events are kept as Content; forgotten events are not removed.
use super::*;
use crate::{
    adapters,
    parser::{required, string},
};
use std::path::PathBuf;

fn source(path: &Path, pointer: &str) -> ImportSource {
    ImportSource::File {
        path: path.into(),
        pointer: pointer.into(),
    }
}

/// Event files ordered by their numeric index (the SDK accepts 5+ digits).
fn events(dir: &Path) -> Result<Vec<PathBuf>, ImportError> {
    let mut found = Vec::new();
    let entries = match std::fs::read_dir(dir) {
        Ok(v) => v,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(ReadError::Io(e).into()),
    };
    for entry in entries {
        let entry = entry.map_err(ReadError::Io)?;
        if !entry.file_type().map_err(ReadError::Io)?.is_file() {
            continue;
        }
        let name = entry.file_name();
        let Some(index) = name
            .to_str()
            .and_then(|n| n.strip_prefix("event-"))
            .filter(|n| n.ends_with(".json"))
            .and_then(|n| n.split_once('-'))
            .and_then(|(idx, _)| idx.parse::<u64>().ok())
        else {
            continue;
        };
        found.push((index, entry.path()));
    }
    found.sort();
    Ok(found.into_iter().map(|(_, path)| path).collect())
}

pub(super) fn import(path: &Path, opts: &ReadOptions) -> Result<SessionImport, ImportError> {
    opts.validate()?;
    if opts.stop_at_byte.is_some() {
        return Err(ReadError::InvalidOptions(
            "multi-file snapshots do not have one byte-prefix boundary",
        )
        .into());
    }
    let dir = if path.file_name().is_some_and(|n| n == "base_state.json") {
        path.parent().unwrap_or(path)
    } else {
        path
    };
    let state_path = dir.join("base_state.json");
    let mut result = SessionImport::default();
    let base = super::opencode_files::load(&state_path, opts, &mut result)?;
    let state = State {
        session_id: string(&base, "id"),
        ..Default::default()
    };
    emit_meta(
        MetaUpdate {
            session_id: state.session_id.clone(),
            cwd: base
                .pointer("/workspace/working_dir")
                .and_then(Value::as_str)
                .map(str::to_owned),
            ..Default::default()
        },
        vec![source(&state_path, "")],
        opts,
        &mut result,
    );
    for path in events(&dir.join("events"))? {
        let event = super::opencode_files::load(&path, opts, &mut result)?;
        let mut p = Parsed {
            include: opts.include,
            record_id: string(&event, "id"),
            timestamp_text: string(&event, "timestamp"),
            ..Default::default()
        };
        parse_event(&event, &mut p)?;
        append(p, &state, vec![source(&path, "")], &mut result);
    }
    ledger(&base, &state_path, opts, &state, &mut result)?;
    result.summary.last_complete_byte = 0;
    result.summary.status = ReadStatus::Complete;
    Ok(result)
}

fn parse_event(v: &Value, p: &mut Parsed) -> Result<(), LineErrorKind> {
    let kind = required(v, "kind")?;
    match kind {
        "MessageEvent" => {
            let m = v
                .get("llm_message")
                .ok_or(LineErrorKind::MissingField("llm_message"))?;
            let role = Role::parse(required(m, "role")?)
                .ok_or_else(|| LineErrorKind::InvalidField("llm_message.role".into()))?;
            p.message_id = string(v, "llm_response_id");
            let content = without_cache_hint(m.get("content").unwrap_or(&Value::Null));
            adapters::message(role, &content, m, p)?;
            reasoning(Some(role), m, p);
            for key in ["tool_calls", "extended_content"] {
                if m.get(key).or_else(|| v.get(key)).is_some_and(nonempty) {
                    adapters::native_content(Some(role), key, v, 4, p);
                }
            }
        }
        "ActionEvent" => {
            p.message_id = string(v, "llm_response_id");
            // `thought` is the assistant text shown alongside the tool call.
            if let Some(thought) = v.get("thought").filter(|t| nonempty(t)) {
                let thought = without_cache_hint(thought);
                adapters::message(Role::Assistant, &thought, &Value::Null, p)?;
            }
            reasoning(Some(Role::Assistant), v, p);
            let call = v.get("tool_call").unwrap_or(&Value::Null);
            adapters::call(
                string(v, "tool_call_id").or_else(|| string(call, "id")),
                string(call, "name")
                    .as_deref()
                    .unwrap_or(required(v, "tool_name")?),
                call.get("arguments"),
                3,
                p,
            );
        }
        "ObservationEvent" => {
            let observation = v
                .get("observation")
                .ok_or(LineErrorKind::MissingField("observation"))?;
            let error = observation.get("is_error").and_then(Value::as_bool);
            adapters::result(string(v, "tool_call_id"), observation, error, 3, p)?;
        }
        "AgentErrorEvent" => {
            let error = v.get("error").ok_or(LineErrorKind::MissingField("error"))?;
            adapters::result(string(v, "tool_call_id"), error, Some(true), 3, p)?;
        }
        "UserRejectObservation" => {
            let reason = v.get("rejection_reason").unwrap_or(&Value::Null);
            adapters::result(string(v, "tool_call_id"), reason, Some(true), 3, p)?;
        }
        // Context, compaction, state, hook and log records keep their payload.
        "SystemPromptEvent"
        | "Condensation"
        | "CondensationRequest"
        | "CondensationSummaryEvent"
        | "ConversationStateUpdateEvent"
        | "ConversationErrorEvent"
        | "HookExecutionEvent"
        | "PauseEvent"
        | "LLMCompletionLogEvent"
        | "ACPToolCallEvent" => adapters::native_content(None, kind, v, 1, p),
        other => p.unknown.push(format!("openhands:{other}")),
    }
    Ok(())
}

/// `cache_prompt` is a provider prompt-caching hint serialized on every content
/// block, not content; dropping it keeps plain text blocks out of Content.
fn without_cache_hint(content: &Value) -> Value {
    let mut content = content.clone();
    if let Some(blocks) = content.as_array_mut() {
        for block in blocks.iter_mut().filter_map(Value::as_object_mut) {
            block.remove("cache_prompt");
        }
    }
    content
}

fn nonempty(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
        _ => true,
    }
}

fn reasoning(role: Option<Role>, v: &Value, p: &mut Parsed) {
    for key in [
        "reasoning_content",
        "thinking_blocks",
        "responses_reasoning_item",
    ] {
        if v.get(key).is_some_and(nonempty) {
            adapters::native_content(role, key, &v[key], 5, p);
        }
    }
}

/// One Usage per recorded LLM call; the accumulated totals are not repeated.
fn ledger(
    base: &Value,
    path: &Path,
    opts: &ReadOptions,
    state: &State,
    result: &mut SessionImport,
) -> Result<(), ImportError> {
    let Some(metrics) = base
        .pointer("/stats/usage_to_metrics")
        .and_then(Value::as_object)
    else {
        return Ok(());
    };
    for (usage_id, m) in metrics {
        let calls = m.get("token_usages").and_then(Value::as_array);
        for (i, u) in calls.into_iter().flatten().enumerate() {
            let mut p = Parsed {
                include: opts.include,
                message_id: string(u, "response_id").filter(|s| !s.is_empty()),
                ..Default::default()
            };
            let counts = TokenCounts {
                input: adapters::counter(u, "prompt_tokens")?,
                output: adapters::counter(u, "completion_tokens")?,
                cache_read: adapters::counter(u, "cache_read_tokens")?,
                cache_write: adapters::counter(u, "cache_write_tokens")?,
                reasoning: adapters::counter(u, "reasoning_tokens")?,
                ..Default::default()
            };
            let key = p.message_id.as_ref().map(|id| format!("openhands:{id}"));
            adapters::usage(
                counts,
                TokenSemantics::Unknown,
                string(u, "model"),
                key,
                &mut p,
            );
            let pointer = format!("/stats/usage_to_metrics/{usage_id}/token_usages/{i}");
            append(p, state, vec![source(path, &pointer)], result);
        }
    }
    Ok(())
}
