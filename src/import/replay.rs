use super::*;
use crate::parser::{required, string};

struct NativeMessage {
    value: Value,
    sources: Vec<ImportSource>,
}
type WireCall = (Value, Vec<ImportSource>, Option<DateTime<Utc>>);
fn invalid(key: &str) -> LineErrorKind {
    LineErrorKind::InvalidField(key.into())
}
fn messages(value: &Value) -> Result<&Vec<Value>, LineErrorKind> {
    value.as_array().ok_or_else(|| invalid("messages"))
}
fn upsert(
    rows: &mut Vec<NativeMessage>,
    v: Value,
    source: ImportSource,
) -> Result<(), LineErrorKind> {
    let id = required(&v, "id")?;
    if let Some(old) = rows
        .iter_mut()
        .find(|r| r.value.get("id").and_then(Value::as_str) == Some(id))
    {
        old.value = v;
        old.sources.push(source);
    } else {
        rows.push(NativeMessage {
            value: v,
            sources: vec![source],
        });
    }
    Ok(())
}
fn patch(
    rows: &mut [NativeMessage],
    patch: &Value,
    source: &ImportSource,
    result: &mut SessionImport,
) -> Result<(), LineErrorKind> {
    let id = required(patch, "id")?;
    let Some(row) = rows
        .iter_mut()
        .find(|r| r.value.get("id").and_then(Value::as_str) == Some(id))
    else {
        tally(
            &mut result.summary.unknown_types,
            "gemini:patch-without-message",
        );
        return Ok(());
    };
    if let Some(content) = patch.get("content") {
        row.value["content"] = content.clone();
    }
    if let Some(calls) = patch.get("toolCalls") {
        for c in messages(calls)? {
            let id = required(c, "id")?;
            if let Some(target) = row
                .value
                .get_mut("toolCalls")
                .and_then(Value::as_array_mut)
                .and_then(|calls| {
                    calls
                        .iter_mut()
                        .find(|c| c.get("id").and_then(Value::as_str) == Some(id))
                })
            {
                if let Some(output) = c.get("result") {
                    target["result"] = output.clone();
                }
            } else {
                tally(
                    &mut result.summary.unknown_types,
                    "gemini:patch-without-tool",
                );
            }
        }
    }
    row.sources.push(source.clone());
    Ok(())
}
pub(super) fn gemini(
    records: &[RawRecord],
    opts: &ReadOptions,
    result: &mut SessionImport,
) -> Result<(), ImportError> {
    let mut rows = Vec::new();
    let mut metadata = serde_json::Map::new();
    let mut metadata_sources = Vec::new();
    for record in records {
        let Some(v) = decode_record(record, opts, result)? else {
            continue;
        };
        let source = record_source(record);
        if let Some(rewind) = v.get("$rewindTo") {
            let id = rewind.as_str().ok_or_else(|| invalid("$rewindTo"))?;
            if let Some(index) = rows
                .iter()
                .position(|r: &NativeMessage| r.value.get("id").and_then(Value::as_str) == Some(id))
            {
                rows.truncate(index + 1);
            } else {
                rows.clear();
            }
            tally(&mut result.summary.ignored_types, "gemini:rewind");
        } else if let Some(p) = v.get("$patch") {
            if !p.is_object() {
                return Err(invalid("$patch").into());
            }
            if p.get("id").is_some() {
                patch(&mut rows, p, &source, result)?;
            }
            if let Some(updates) = p.get("updates") {
                for update in messages(updates)? {
                    patch(&mut rows, update, &source, result)?;
                }
            }
            if let Some(ids) = p.get("removeIds") {
                for id in messages(ids)? {
                    let id = id.as_str().ok_or_else(|| invalid("removeIds"))?;
                    rows.retain(|r| r.value.get("id").and_then(Value::as_str) != Some(id));
                }
            }
            if let Some(ids) = p.get("orderIds") {
                let mut ordered = Vec::new();
                for id in messages(ids)? {
                    let id = id.as_str().ok_or_else(|| invalid("orderIds"))?;
                    if let Some(index) = rows
                        .iter()
                        .position(|r| r.value.get("id").and_then(Value::as_str) == Some(id))
                    {
                        ordered.push(rows.remove(index));
                    }
                }
                rows.extend(ordered);
            }
            for key in p.as_object().unwrap().keys() {
                if !matches!(
                    key.as_str(),
                    "id" | "content" | "toolCalls" | "updates" | "removeIds" | "orderIds"
                ) {
                    tally(
                        &mut result.summary.unknown_types,
                        &format!("gemini:patch:{key}"),
                    );
                }
            }
        } else if let Some(set) = v.get("$set") {
            let set = set.as_object().ok_or_else(|| invalid("$set"))?;
            if let Some(ms) = set.get("messages") {
                rows.clear();
                for m in messages(ms)? {
                    upsert(&mut rows, m.clone(), source.clone())?;
                }
            }
            metadata.extend(
                set.iter()
                    .filter(|(k, _)| *k != "messages")
                    .map(|(k, v)| (k.clone(), v.clone())),
            );
            metadata_sources.push(source);
        } else if v.get("sessionId").is_some() && v.get("projectHash").is_some() {
            let map = v.as_object().ok_or_else(|| invalid("metadata"))?;
            metadata.extend(
                map.iter()
                    .filter(|(k, _)| *k != "messages")
                    .map(|(k, v)| (k.clone(), v.clone())),
            );
            metadata_sources.push(source.clone());
            if let Some(ms) = v.get("messages") {
                for m in messages(ms)? {
                    upsert(&mut rows, m.clone(), source.clone())?;
                }
            }
        } else if v.get("id").is_some() {
            upsert(&mut rows, v, source)?;
        } else {
            tally(&mut result.summary.unknown_types, "gemini:record");
        }
    }
    let mut state = State {
        session_id: metadata
            .get("sessionId")
            .and_then(Value::as_str)
            .map(str::to_owned),
        sidechain: metadata.get("kind").and_then(Value::as_str) == Some("subagent"),
        ..Default::default()
    };
    if state.session_id.is_none() {
        return Err(LineErrorKind::MissingField("sessionId").into());
    }
    emit_meta(
        MetaUpdate {
            session_id: state.session_id.clone(),
            ..Default::default()
        },
        metadata_sources,
        opts,
        result,
    );
    for row in rows {
        parse_record(
            Agent::GeminiCli,
            &row.value,
            &mut state,
            row.sources,
            opts,
            result,
        )?;
    }
    Ok(())
}
fn flush_text(
    text: &mut String,
    sources: &mut Vec<ImportSource>,
    at: &mut Option<DateTime<Utc>>,
    opts: &ReadOptions,
    result: &mut SessionImport,
) -> Result<(), ImportError> {
    if sources.is_empty() {
        return Ok(());
    }
    let mut p = Parsed {
        include: opts.include,
        at: *at,
        ..Default::default()
    };
    crate::adapters::message(
        Role::Assistant,
        &Value::String(std::mem::take(text)),
        &Value::Null,
        &mut p,
    )?;
    append(p, &State::default(), std::mem::take(sources), result);
    *at = None;
    Ok(())
}
fn flush_call(
    call: &mut Option<WireCall>,
    opts: &ReadOptions,
    result: &mut SessionImport,
) -> Result<(), ImportError> {
    if let Some((v, sources, at)) = call.take() {
        let f = v
            .get("function")
            .ok_or(LineErrorKind::MissingField("function"))?;
        let mut p = Parsed {
            include: opts.include,
            at,
            ..Default::default()
        };
        if p.wants(EventKinds::TOOL_CALL) {
            crate::adapters::call(
                string(&v, "id"),
                required(f, "name")?,
                f.get("arguments"),
                3,
                &mut p,
            );
        }
        append(p, &State::default(), sources, result);
    }
    Ok(())
}
pub(super) fn kimi(
    records: &[RawRecord],
    opts: &ReadOptions,
    result: &mut SessionImport,
) -> Result<(), ImportError> {
    let mut state = State::default();
    let mut text = String::new();
    let mut text_sources = Vec::new();
    let mut at = None;
    let mut call: Option<WireCall> = None;
    let mut journal = Vec::new();
    let mut subagents =
        std::collections::BTreeMap::<String, (Option<String>, Vec<RawRecord>)>::new();
    for record in records {
        let Some(v) = decode_record(record, opts, result)? else {
            continue;
        };
        let source = record_source(record);
        if v.get("role").is_some() {
            parse_record(Agent::KimiCli, &v, &mut state, vec![source], opts, result)?;
            continue;
        }
        if v.get("type").and_then(Value::as_str) == Some("metadata") {
            tally(&mut result.summary.ignored_types, "kimi:wire-metadata");
            continue;
        }
        // Kimi Code v2 journals have top-level dotted record types, unlike the
        // earlier {timestamp, message:{type,payload}} wire envelope.
        if v.get("type").is_some() {
            journal.push((v, vec![source]));
            continue;
        }
        let envelope = v
            .get("message")
            .ok_or(LineErrorKind::MissingField("message"))?;
        let kind = required(envelope, "type")?;
        let payload = envelope
            .get("payload")
            .ok_or(LineErrorKind::MissingField("payload"))?;
        let timestamp = crate::adapters::seconds(v.get("timestamp"))?;
        let mut p = Parsed {
            include: opts.include,
            at: timestamp,
            ..Default::default()
        };
        match kind {
            "TurnBegin" | "SteerInput" => {
                flush_text(&mut text, &mut text_sources, &mut at, opts, result)?;
                flush_call(&mut call, opts, result)?;
                crate::adapters::message(
                    Role::User,
                    payload
                        .get("user_input")
                        .ok_or(LineErrorKind::MissingField("user_input"))?,
                    payload,
                    &mut p,
                )?;
            }
            "ContentPart" => {
                if payload.get("type").and_then(Value::as_str) == Some("text") {
                    if !p.wants(EventKinds::MESSAGE) {
                        continue;
                    }
                    if text_sources.is_empty() {
                        at = timestamp;
                    }
                    text.push_str(required(payload, "text")?);
                    text_sources.push(source.clone());
                } else {
                    // Use the same content diagnostics for media / thinking / future parts.
                    if p.wants(EventKinds::MESSAGE) || p.wants(EventKinds::CONTENT) {
                        crate::adapters::parts(
                            &Value::Array(vec![payload.clone()]),
                            &mut p,
                            Some(Role::Assistant),
                        )?;
                    }
                }
            }
            "ToolCall" => {
                flush_text(&mut text, &mut text_sources, &mut at, opts, result)?;
                flush_call(&mut call, opts, result)?;
                if p.wants(EventKinds::TOOL_CALL) {
                    call = Some((payload.clone(), vec![source.clone()], timestamp));
                }
            }
            "ToolCallPart" if p.wants(EventKinds::TOOL_CALL) => {
                let (c, sources, _) = call
                    .as_mut()
                    .ok_or(LineErrorKind::MissingField("preceding ToolCall"))?;
                if let Some(part) = payload.get("arguments_part").filter(|v| !v.is_null()) {
                    let part = part.as_str().ok_or_else(|| invalid("arguments_part"))?;
                    let args = c
                        .pointer("/function/arguments")
                        .filter(|v| !v.is_null())
                        .map(|v| v.as_str().ok_or_else(|| invalid("arguments")))
                        .transpose()?
                        .unwrap_or("");
                    c["function"]["arguments"] = Value::String(format!("{args}{part}"));
                }
                sources.push(source.clone());
            }
            "ToolResult" => {
                flush_text(&mut text, &mut text_sources, &mut at, opts, result)?;
                flush_call(&mut call, opts, result)?;
                if p.wants(EventKinds::TOOL_RESULT) {
                    let r = payload
                        .get("return_value")
                        .ok_or(LineErrorKind::MissingField("return_value"))?;
                    crate::adapters::result(
                        string(payload, "tool_call_id"),
                        r.get("output")
                            .ok_or(LineErrorKind::MissingField("output"))?,
                        r.get("is_error").and_then(Value::as_bool),
                        3,
                        &mut p,
                    )?;
                }
            }
            "SubagentEvent" => {
                let event = payload
                    .get("event")
                    .ok_or(LineErrorKind::MissingField("SubagentEvent.event"))?;
                let parent = string(payload, "parent_tool_call_id")
                    .or_else(|| string(payload, "task_tool_call_id"));
                let id = string(payload, "agent_id")
                    .or_else(|| parent.clone())
                    .unwrap_or_default();
                let mut nested = record.clone();
                nested.bytes = serde_json::to_vec(
                    &serde_json::json!({"timestamp":v.get("timestamp"), "message":event}),
                )
                .map_err(|_| invalid("SubagentEvent"))?;
                subagents
                    .entry(id)
                    .or_insert_with(|| (parent, Vec::new()))
                    .1
                    .push(nested);
                crate::adapters::native_content(None, "SubagentEvent", payload, 1, &mut p);
            }
            "StepBegin" | "TurnEnd" | "StepInterrupted" => {
                flush_text(&mut text, &mut text_sources, &mut at, opts, result)?;
                flush_call(&mut call, opts, result)?;
                p.ignored.push(format!("kimi:{kind}"));
            }
            "StatusUpdate" => {
                if p.wants(EventKinds::USAGE)
                    && let Some(u) = payload.get("token_usage").filter(|u| !u.is_null())
                {
                    let counts = TokenCounts {
                        input: crate::adapters::counter(u, "input_other")?,
                        output: crate::adapters::counter(u, "output")?,
                        cache_read: crate::adapters::counter(u, "input_cache_read")?,
                        cache_write: crate::adapters::counter(u, "input_cache_creation")?,
                        ..Default::default()
                    };
                    p.emit(
                        2,
                        Event::Usage(Usage {
                            counts,
                            semantics: TokenSemantics::Unknown,
                            dedup_key: string(payload, "message_id"),
                            model: None,
                            cumulative: None,
                            basis: UsageBasis::Message,
                            stop_reason: None,
                            endpoint: Endpoint::Unknown,
                            inference_geo: None,
                            adjustments: Vec::new(),
                        }),
                    );
                }
            }
            "ToolCallPart" | "StepRetry" | "CompactionBegin" | "CompactionEnd"
            | "ApprovalRequest" | "ApprovalResponse" | "MCPLoadingBegin" | "MCPLoadingEnd" => {
                p.ignored.push(format!("kimi:{kind}"))
            }
            other => p.unknown.push(format!("kimi:{other}")),
        }
        append(p, &state, vec![source], result);
    }
    flush_text(&mut text, &mut text_sources, &mut at, opts, result)?;
    flush_call(&mut call, opts, result)?;
    super::kimi_v2::fold(journal, opts, result)?;
    for (_, (parent, records)) in subagents {
        let start = result.events.len();
        kimi(&records, opts, result)?;
        for e in &mut result.events[start..] {
            if let Event::Message(m) = &mut e.value {
                m.is_sidechain = true;
                m.parent_id = parent.clone();
            }
        }
    }
    Ok(())
}

pub(super) fn kimi_code_record(
    v: &Value,
    state: &mut State,
    opts: &ReadOptions,
    sources: Vec<ImportSource>,
    result: &mut SessionImport,
) -> Result<(), ImportError> {
    let mut p = Parsed {
        include: opts.include,
        at: crate::adapters::millis(v.get("time"))?,
        ..Default::default()
    };
    let kind = required(v, "type")?;
    match kind {
        "context.append_message" => {
            if !p.wants(EventKinds::MESSAGE)
                && !p.wants(EventKinds::CONTENT)
                && !p.wants(EventKinds::TOOL_CALL)
                && !p.wants(EventKinds::TOOL_RESULT)
            {
                p.ignored.push("kimi:v2:context.append_message".into());
                append(p, state, sources, result);
                return Ok(());
            }
            let m = v
                .get("message")
                .ok_or(LineErrorKind::MissingField("message"))?;
            let role = required(m, "role")?;
            if role == "tool" {
                crate::adapters::api_message(m, &mut p)?;
            } else if let Some(role) = Role::parse(role) {
                crate::adapters::message(
                    role,
                    m.get("content")
                        .ok_or(LineErrorKind::MissingField("content"))?,
                    m,
                    &mut p,
                )?;
                if p.wants(EventKinds::TOOL_CALL)
                    && let Some(calls) = m.get("toolCalls")
                {
                    for (i, c) in messages(calls)?.iter().enumerate() {
                        crate::adapters::call(
                            string(c, "id"),
                            required(c, "name")?,
                            c.get("arguments"),
                            3 + m["content"].as_array().map_or(0, Vec::len) + i,
                            &mut p,
                        );
                    }
                }
            } else {
                p.unknown.push(format!("role:{role}"));
            }
            p.message_id = string(m, "id").or_else(|| string(m, "providerMessageId"));
        }
        "context.append_loop_event" => {
            let e = v.get("event").ok_or(LineErrorKind::MissingField("event"))?;
            p.record_id = string(e, "uuid");
            p.message_id = string(e, "stepUuid");
            match required(e, "type")? {
                "content.part" if p.wants(EventKinds::MESSAGE) || p.wants(EventKinds::CONTENT) => {
                    let part = e.get("part").ok_or(LineErrorKind::MissingField("part"))?;
                    crate::adapters::message(
                        Role::Assistant,
                        &Value::Array(vec![part.clone()]),
                        e,
                        &mut p,
                    )?;
                }
                "tool.call" if p.wants(EventKinds::TOOL_CALL) => {
                    crate::adapters::call(
                        string(e, "toolCallId"),
                        required(e, "name")?,
                        e.get("args"),
                        3,
                        &mut p,
                    );
                }
                "tool.result" if p.wants(EventKinds::TOOL_RESULT) => {
                    let r = e
                        .get("result")
                        .ok_or(LineErrorKind::MissingField("result"))?;
                    crate::adapters::result(
                        string(e, "toolCallId"),
                        r.get("output")
                            .ok_or(LineErrorKind::MissingField("output"))?,
                        r.get("isError").and_then(Value::as_bool),
                        3,
                        &mut p,
                    )?;
                }
                // step.end usage mirrors usage.record, which is the canonical ledger.
                "step.begin" | "step.end" | "step.retry" | "tool.call" | "tool.result"
                | "content.part" => {
                    p.ignored.push(format!("kimi:v2:{}", required(e, "type")?));
                }
                other => p.unknown.push(format!("kimi:v2:loop:{other}")),
            }
        }
        "usage.record" if p.wants(EventKinds::USAGE) => {
            let u = v.get("usage").ok_or(LineErrorKind::MissingField("usage"))?;
            p.emit(
                2,
                Event::Usage(Usage {
                    counts: TokenCounts {
                        input: crate::adapters::counter(u, "inputOther")?,
                        output: crate::adapters::counter(u, "output")?,
                        cache_read: crate::adapters::counter(u, "inputCacheRead")?,
                        cache_write: crate::adapters::counter(u, "inputCacheCreation")?,
                        ..Default::default()
                    },
                    semantics: TokenSemantics::Unknown,
                    model: string(v, "model"),
                    dedup_key: None,
                    cumulative: None,
                    basis: UsageBasis::Message,
                    stop_reason: None,
                    endpoint: Endpoint::Unknown,
                    inference_geo: None,
                    adjustments: Vec::new(),
                }),
            );
        }
        "llm.request" => {
            state.model = string(v, "model");
            p.emit(
                0,
                Event::Meta(MetaUpdate {
                    model: state.model.clone(),
                    ..Default::default()
                }),
            );
        }
        "config.update"
        | "tools.set_active_tools"
        | "permission.set_mode"
        | "llm.tools_snapshot"
        | "tools.update_store"
        | "turn.prompt"
        | "turn.cancel"
        | "turn.ended"
        | "usage.record"
        | "context.clear"
        | "context.undo" => p.ignored.push(format!("kimi:v2:{kind}")),
        other => p.unknown.push(format!("kimi:v2:{other}")),
    }
    append(p, state, sources, result);
    Ok(())
}

/// Resolve Pi's current leaf, context edits and newest compaction. The streaming
/// reader continues to expose the append-only tree; snapshots expose active context.
pub(super) fn pi(
    records: &[RawRecord],
    opts: &ReadOptions,
    result: &mut SessionImport,
) -> Result<(), ImportError> {
    use std::collections::{HashMap, HashSet};
    let mut rows = Vec::new();
    for record in records {
        if let Some(v) = decode_record(record, opts, result)? {
            rows.push(NativeMessage {
                value: v,
                sources: vec![record_source(record)],
            });
        }
    }
    let linear = rows
        .iter()
        .filter(|r| r.value["type"] != "session")
        .all(|r| r.value.get("id").is_none())
        && rows
            .iter()
            .filter(|r| r.value["type"] == "session")
            .all(|r| r.value["version"].as_u64().unwrap_or(1) < 2);
    let by_id: HashMap<_, _> = rows
        .iter()
        .enumerate()
        .filter(|(_, r)| r.value["type"] != "session")
        .filter_map(|(i, r)| string(&r.value, "id").map(|id| (id, i)))
        .collect();
    let mut path = Vec::new();
    let mut seen = HashSet::new();
    let mut current = rows.iter().rposition(|r| r.value["type"] != "session");
    while let Some(i) = current {
        if !seen.insert(i) {
            return Err(invalid("Pi parent cycle").into());
        }
        path.push(i);
        current = match rows[i].value.get("parentId").filter(|v| !v.is_null()) {
            Some(Value::String(id)) => {
                Some(*by_id.get(id).ok_or_else(|| invalid("Pi missing parent"))?)
            }
            None => None,
            _ => return Err(invalid("Pi parentId").into()),
        };
    }
    path.reverse();
    if linear {
        path = rows
            .iter()
            .enumerate()
            .filter(|(_, r)| r.value["type"] != "session")
            .map(|(i, _)| i)
            .collect();
    }
    // A separate physical ledger survives branch changes and compaction.
    let mut state = State::default();
    let active: HashSet<_> = path.iter().copied().collect();
    for (i, row) in rows.iter().enumerate() {
        let p = crate::parser::parse(
            Agent::Pi,
            &row.value,
            &mut state,
            opts.codex_usage,
            if opts.include.contains(EventKinds::USAGE) {
                EventKinds::USAGE
            } else {
                EventKinds::META
            },
            opts.accounting,
        )?;
        let mut p = p;
        p.events.retain(|(_, e)| matches!(e, Event::Usage(_)));
        // Inactive branches still contribute diagnostics and billed usage.
        // Active rows are parsed below with the requested projections.
        if active.contains(&i) || row.value["type"] == "session" {
            p.unknown.clear();
        }
        p.ignored.clear();
        append(p, &state, row.sources.clone(), result);
    }
    if let Some(compaction) = path
        .iter()
        .rposition(|i| rows[*i].value["type"] == "compaction")
    {
        let index = path[compaction];
        let kept = if linear {
            let first = rows[index].value["firstKeptEntryIndex"]
                .as_u64()
                .ok_or_else(|| invalid("firstKeptEntryIndex"))?;
            path[..compaction].iter().position(|i| *i as u64 == first)
        } else {
            let first = required(&rows[index].value, "firstKeptEntryId")?;
            path[..compaction]
                .iter()
                .position(|i| rows[*i].value["id"] == first)
        };
        let mut active = vec![index];
        if let Some(kept) = kept {
            active.extend_from_slice(&path[kept..compaction]);
        }
        active.extend_from_slice(&path[compaction + 1..]);
        path = active;
    }
    let mut edits = HashMap::new();
    for &i in &path {
        if rows[i].value["type"] == "context_edit" {
            edits.insert(required(&rows[i].value, "targetId")?.to_owned(), i);
        }
    }
    state = State::default();
    for i in rows
        .iter()
        .enumerate()
        .filter(|(_, r)| r.value["type"] == "session")
        .map(|(i, _)| i)
        .chain(path)
    {
        let row = &rows[i];
        let mut v = row.value.clone();
        let mut sources = row.sources.clone();
        if let Some(edit) = string(&v, "id").and_then(|id| edits.get(&id)) {
            let replacement = rows[*edit]
                .value
                .get("replacement")
                .ok_or(LineErrorKind::MissingField("replacement"))?;
            if replacement.is_null() {
                continue;
            }
            let content = replacement
                .get("content")
                .ok_or(LineErrorKind::MissingField("replacement.content"))?;
            if v["type"] == "message" {
                v["message"]["content"] = content.clone();
            } else if v["type"] == "custom_message" {
                v["content"] = content.clone();
            }
            sources.extend(rows[*edit].sources.clone());
        }
        let mut p = crate::parser::parse(
            Agent::Pi,
            &v,
            &mut state,
            opts.codex_usage,
            opts.include,
            opts.accounting,
        )?;
        p.events.retain(|(_, e)| !matches!(e, Event::Usage(_)));
        append(p, &state, sources, result);
    }
    Ok(())
}

pub(super) fn codebuddy(
    records: &[RawRecord],
    opts: &ReadOptions,
    result: &mut SessionImport,
) -> Result<(), ImportError> {
    use std::collections::{HashMap, HashSet};
    let mut rows: Vec<NativeMessage> = Vec::new();
    let mut upserts = HashMap::new();
    for record in records {
        let Some(v) = decode_record(record, opts, result)? else {
            continue;
        };
        let native = v
            .get("payload")
            .filter(|v| v.is_object())
            .unwrap_or(&v)
            .clone();
        let id = string(&v, "uuid");
        if let Some(&index) = id.as_ref().and_then(|id| upserts.get(id)) {
            let row: &mut NativeMessage = &mut rows[index];
            row.value = native;
            row.sources.push(record_source(record));
        } else {
            if let Some(id) = id {
                upserts.insert(id, rows.len());
            }
            rows.push(NativeMessage {
                value: native,
                sources: vec![record_source(record)],
            });
        }
    }
    fn fact(v: &Value) -> bool {
        matches!(
            v["type"].as_str(),
            Some(
                "custom-title"
                    | "ai-title"
                    | "file-history-snapshot"
                    | "summary"
                    | "topic"
                    | "goal-result"
                    | "goal-progress"
                    | "turn-metrics"
                    | "resend-fork-notice"
                    | "acp-terminal-state"
                    | "model-usage"
                    | "credit-usage"
                    | "session-meta"
            )
        )
    }
    let branched = rows.iter().any(|r| r.value["type"] == "resend-fork-notice");
    let mut selected = HashSet::new();
    if branched {
        let mut by_id = HashMap::new();
        for (i, row) in rows.iter().enumerate().filter(|(_, r)| !fact(&r.value)) {
            if let Some(id) = string(&row.value, "id")
                && by_id.insert(id, i).is_some()
            {
                return Err(invalid("CodeBuddy ambiguous branch id").into());
            }
        }
        let mut tip = None;
        for row in &rows {
            if row.value["type"] == "resend-fork-notice" {
                tip = string(&row.value, "parentId");
            } else if !fact(&row.value) {
                tip = string(&row.value, "id");
            }
        }
        while let Some(id) = tip {
            let index = *by_id
                .get(&id)
                .ok_or_else(|| invalid("CodeBuddy missing branch parent"))?;
            if !selected.insert(index) {
                return Err(invalid("CodeBuddy parent cycle").into());
            }
            tip = string(&rows[index].value, "parentId");
        }
    } else {
        selected.extend(0..rows.len());
    }
    // /clear is an explicit session separator, independent of fork metadata.
    if let Some(clear) = rows.iter().rposition(|r| {
        r.value
            .pointer("/providerData/isSessionSeparator")
            .and_then(Value::as_bool)
            == Some(true)
    }) {
        selected.retain(|i| *i > clear);
    }
    let mut state = State::default();
    for (i, row) in rows.into_iter().enumerate() {
        let include = if selected.contains(&i) || fact(&row.value) {
            opts.include
        } else if opts.include.contains(EventKinds::USAGE) {
            EventKinds::USAGE
        } else {
            EventKinds::META
        };
        let mut p = crate::parser::parse(
            Agent::CodeBuddy,
            &row.value,
            &mut state,
            opts.codex_usage,
            include,
            opts.accounting,
        )?;
        if !selected.contains(&i) && !fact(&row.value) {
            p.events.retain(|(_, e)| matches!(e, Event::Usage(_)));
        }
        append(p, &state, row.sources, result);
    }
    Ok(())
}
