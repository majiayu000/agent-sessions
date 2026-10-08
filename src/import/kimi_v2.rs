//! Kimi Code's context journal. Billing remains a physical ledger; conversation
//! messages are folded before rendering so undo/clear cannot leak stale text.
use super::*;
use crate::parser::{required, string};
use std::collections::{BTreeMap, HashSet};

#[derive(Clone)]
struct MessageRow {
    message: Value,
    time: Option<Value>,
    sources: Vec<ImportSource>,
}
#[derive(Default)]
struct Context {
    messages: Vec<MessageRow>,
    open: Option<usize>,
    step: Option<String>,
    pending: HashSet<String>,
    deferred: Vec<MessageRow>,
}
impl Context {
    fn flush(&mut self) {
        self.messages.append(&mut self.deferred);
    }
    fn settle(&mut self) {
        self.open = None;
        self.step = None;
        self.pending.clear();
        self.flush();
    }
    fn reset(&mut self) {
        self.open = None;
        self.step = None;
        self.pending.clear();
        self.deferred.clear();
    }
    fn push(&mut self, row: MessageRow) {
        if self.pending.is_empty() {
            self.messages.push(row);
        } else {
            self.deferred.push(row);
        }
    }
}
fn anchor(m: &Value) -> bool {
    m["role"] == "user"
        && (m.get("origin").is_none_or(Value::is_null)
            || m.pointer("/origin/kind").and_then(Value::as_str) == Some("user")
            || (matches!(
                m.pointer("/origin/kind").and_then(Value::as_str),
                Some("skill_activation" | "plugin_command")
            ) && m.pointer("/origin/trigger").and_then(Value::as_str) == Some("user-slash")))
}
fn invalid(key: &str) -> ImportError {
    LineErrorKind::InvalidField(key.into()).into()
}

// Same Unicode token estimate used by the native compaction fold. These are
// context selection budgets, never reported as billable Usage.
fn estimate(text: &str) -> usize {
    let ascii = text.chars().filter(char::is_ascii).count();
    ascii.div_ceil(4) + text.chars().count() - ascii
}
fn text_of(m: &Value) -> String {
    m["content"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|p| p["type"] == "text")
        .filter_map(|p| p["text"].as_str())
        .collect()
}
fn message_tokens(m: &Value) -> usize {
    let mut tokens = estimate(m["role"].as_str().unwrap_or_default());
    for p in m["content"].as_array().into_iter().flatten() {
        tokens += match p["type"].as_str() {
            Some("text") => estimate(p["text"].as_str().unwrap_or_default()),
            Some("think") => estimate(p["think"].as_str().unwrap_or_default()),
            Some("image_url" | "audio_url" | "video_url") => 2000,
            _ => 0,
        };
    }
    for c in m["toolCalls"].as_array().into_iter().flatten() {
        tokens += estimate(c["name"].as_str().unwrap_or_default())
            + estimate(&c["arguments"].to_string());
    }
    tokens
}
fn truncate(text: &str, budget: usize, suffix: bool) -> String {
    let mut chars: Vec<_> = text.chars().collect();
    if suffix {
        chars.reverse();
    }
    let (mut ascii, mut other, mut end) = (0_usize, 0_usize, 0);
    for c in &chars {
        if c.is_ascii() {
            ascii += 1;
        } else {
            other += 1;
        }
        if ascii.div_ceil(4) + other > budget {
            break;
        }
        end += 1;
    }
    chars.truncate(end);
    if suffix {
        chars.reverse();
    }
    chars.into_iter().collect()
}
fn replace_text(row: &MessageRow, text: String) -> MessageRow {
    let mut row = row.clone();
    row.message["content"] = serde_json::json!([{"type":"text", "text":text}]);
    row.message["toolCalls"] = serde_json::json!([]);
    row
}
fn compact_users(mut rows: Vec<MessageRow>) -> Vec<MessageRow> {
    rows.retain(|r| anchor(&r.message));
    if rows
        .iter()
        .map(|r| message_tokens(&r.message))
        .sum::<usize>()
        <= 20_000
    {
        return rows;
    }
    let (mut tail, mut remaining, mut head_end, mut boundary) =
        (Vec::new(), 18_000, rows.len(), None);
    for (i, r) in rows.iter().enumerate().rev() {
        if remaining == 0 {
            break;
        }
        head_end = i;
        let tokens = message_tokens(&r.message);
        if tokens <= remaining {
            tail.push(r.clone());
            remaining -= tokens;
        } else {
            let text = text_of(&r.message);
            let suffix = truncate(&text, remaining, true);
            boundary = Some(replace_text(r, text[..text.len() - suffix.len()].into()));
            tail.push(replace_text(r, suffix));
            break;
        }
    }
    tail.reverse();
    let mut candidates = rows[..head_end].to_vec();
    if let Some(r) = boundary {
        candidates.push(r);
    }
    let (mut head, mut remaining) = (Vec::new(), 2000);
    for r in candidates {
        if remaining == 0 {
            break;
        }
        let tokens = message_tokens(&r.message);
        if tokens <= remaining {
            remaining -= tokens;
            head.push(r);
        } else {
            head.push(replace_text(
                &r,
                truncate(&text_of(&r.message), remaining, false),
            ));
            break;
        }
    }
    head.extend(tail);
    head
}

pub(super) fn fold(
    records: Vec<(Value, Vec<ImportSource>)>,
    opts: &ReadOptions,
    result: &mut SessionImport,
) -> Result<(), ImportError> {
    let mut contexts = BTreeMap::<String, Context>::new();
    let mut state = State::default();
    for (v, sources) in records {
        let kind = required(&v, "type")?;
        let agent = string(&v, "agentId").unwrap_or_default();
        let ctx = contexts.entry(agent).or_default();
        let time = v.get("time").cloned();
        if kind.starts_with("context.")
            && !opts.include.contains(EventKinds::MESSAGE)
            && !opts.include.contains(EventKinds::CONTENT)
            && !opts.include.contains(EventKinds::TOOL_CALL)
            && !opts.include.contains(EventKinds::TOOL_RESULT)
        {
            continue;
        }
        if matches!(
            kind,
            "context.undo" | "context.clear" | "context.apply_compaction"
        ) {
            let mut p = Parsed {
                include: opts.include,
                at: crate::adapters::millis(v.get("time"))?,
                ..Default::default()
            };
            crate::adapters::native_content(None, kind, &v, 1, &mut p);
            append(p, &state, sources.clone(), result);
        }
        match kind {
            "context.append_message" => {
                // A usage-only import does not validate excluded bodies.
                if !opts.include.contains(EventKinds::MESSAGE)
                    && !opts.include.contains(EventKinds::CONTENT)
                    && !opts.include.contains(EventKinds::TOOL_CALL)
                    && !opts.include.contains(EventKinds::TOOL_RESULT)
                {
                    continue;
                }
                let message = v
                    .get("message")
                    .ok_or(LineErrorKind::MissingField("message"))?
                    .clone();
                ctx.push(MessageRow {
                    message,
                    time,
                    sources,
                });
            }
            "context.append_loop_event" => {
                let e = v.get("event").ok_or(LineErrorKind::MissingField("event"))?;
                match required(e, "type")? {
                    "step.begin" => {
                        ctx.settle();
                        ctx.step = string(e, "uuid");
                        ctx.open = Some(ctx.messages.len());
                        ctx.messages.push(MessageRow {
                            message: serde_json::json!({"role":"assistant", "id":ctx.step, "content":[], "toolCalls":[]}),
                            time, sources,
                        });
                    }
                    "content.part" | "tool.call" => {
                        let step = string(e, "stepUuid");
                        // Like the native fold, reject late fragments from a
                        // closed/undone step instead of reviving stale context.
                        if ctx.open.is_none() || step.is_none() || step != ctx.step {
                            tally(
                                &mut result.summary.ignored_types,
                                "kimi:v2:inactive-step-fragment",
                            );
                            continue;
                        }
                        let row = &mut ctx.messages[ctx.open.unwrap()];
                        if e["type"] == "content.part" {
                            row.message["content"].as_array_mut().unwrap().push(
                                e.get("part")
                                    .ok_or(LineErrorKind::MissingField("part"))?
                                    .clone(),
                            );
                            // Timestamp of the first content, not step bookkeeping.
                            if row.message["content"].as_array().unwrap().len() == 1 {
                                row.time = time;
                            }
                        } else {
                            let id = required(e, "toolCallId")?;
                            ctx.pending.insert(id.into());
                            row.message["toolCalls"].as_array_mut().unwrap().push(
                                serde_json::json!({
                                    "id":id, "name":required(e, "name")?, "arguments":e.get("args")
                                }),
                            );
                        }
                        row.sources.extend(sources);
                    }
                    "tool.result" => {
                        let id = required(e, "toolCallId")?;
                        if !ctx.pending.remove(id) {
                            tally(
                                &mut result.summary.ignored_types,
                                "kimi:v2:inactive-tool-result",
                            );
                            continue;
                        }
                        let r = e
                            .get("result")
                            .ok_or(LineErrorKind::MissingField("result"))?;
                        ctx.messages.push(MessageRow {
                            message: serde_json::json!({"role":"tool", "toolCallId":id,
                                "content":r.get("output").ok_or(LineErrorKind::MissingField("output"))?,
                                "isError":r.get("isError"), "note":r.get("note")}),
                            time, sources,
                        });
                        if ctx.pending.is_empty() {
                            ctx.flush();
                        }
                    }
                    "step.end" => {
                        if !matches!(
                            e.get("finishReason").and_then(Value::as_str),
                            Some("interrupted" | "error")
                        ) {
                            ctx.settle();
                        }
                        // usage.record is the canonical billing ledger.
                    }
                    "step.retry" => {}
                    other => tally(
                        &mut result.summary.unknown_types,
                        &format!("kimi:v2:loop:{other}"),
                    ),
                }
            }
            "context.clear" => {
                ctx.messages.clear();
                ctx.deferred.clear();
                ctx.settle();
            }
            "context.undo" => {
                let count = v
                    .get("count")
                    .and_then(Value::as_u64)
                    .filter(|n| *n > 0)
                    .ok_or_else(|| invalid("undo count"))?;
                let mut remaining = count;
                let mut cut = None;
                for (i, row) in ctx.messages.iter().enumerate().rev() {
                    if row.message.pointer("/origin/kind").and_then(Value::as_str)
                        == Some("compaction_summary")
                    {
                        break;
                    }
                    if anchor(&row.message) {
                        remaining -= 1;
                        cut = Some(i);
                        if remaining == 0 {
                            break;
                        }
                    }
                }
                if remaining == 0 {
                    let mut cut = cut.unwrap();
                    let id = string(&ctx.messages[cut].message, "id");
                    while cut > 0
                        && id.is_some()
                        && ctx.messages[cut - 1]
                            .message
                            .pointer("/origin/kind")
                            .and_then(Value::as_str)
                            == Some("injection")
                        && ctx.messages[cut - 1]
                            .message
                            .pointer("/origin/ownerPromptId")
                            .and_then(Value::as_str)
                            == id.as_deref()
                    {
                        cut -= 1;
                    }
                    ctx.messages.truncate(cut);
                    ctx.reset();
                }
            }
            "context.apply_compaction" => {
                let count = v
                    .get("compactedCount")
                    .or_else(|| v.get("count"))
                    .and_then(Value::as_u64)
                    .ok_or_else(|| invalid("compactedCount"))?;
                let summary = v
                    .get("contextSummary")
                    .or_else(|| v.get("summary"))
                    .ok_or(LineErrorKind::MissingField("summary"))?;
                let message = if summary.is_string() {
                    serde_json::json!({"role":"user",
                    "content":[{"type":"text", "text":summary}], "origin":{"kind":"compaction_summary"}})
                } else if summary.get("role").is_some() {
                    summary.clone()
                } else {
                    return Err(invalid("compaction summary"));
                };
                let row = MessageRow {
                    message,
                    time,
                    sources,
                };
                let legacy = v
                    .get("legacyTail")
                    .and_then(Value::as_bool)
                    .unwrap_or(v.get("keptUserMessageCount").is_none());
                if legacy {
                    let tail = ctx
                        .messages
                        .split_off((count.min(ctx.messages.len() as u64)) as usize);
                    ctx.messages = vec![row];
                    ctx.messages.extend(tail);
                } else {
                    // User prompts survive current compaction. Preserve the
                    // native summary and explicit compaction record as evidence.
                    ctx.messages = compact_users(std::mem::take(&mut ctx.messages));
                    ctx.messages.push(row);
                }
                ctx.reset();
            }
            _ => super::replay::kimi_code_record(&v, &mut state, opts, sources, result)?,
        }
    }
    for (agent, mut ctx) in contexts {
        ctx.settle();
        if let Some(row) = ctx.messages.first() {
            emit_meta(
                MetaUpdate {
                    source: Some(serde_json::json!({"agentId":agent})),
                    ..Default::default()
                },
                row.sources.clone(),
                opts,
                result,
            );
        }
        for row in ctx.messages {
            // Empty assistant steps contain no conversation contribution.
            if row.message["role"] == "assistant"
                && row.message["content"].as_array().is_some_and(Vec::is_empty)
                && row.message["toolCalls"]
                    .as_array()
                    .is_some_and(Vec::is_empty)
            {
                continue;
            }
            let v = serde_json::json!({"type":"context.append_message", "message":row.message, "time":row.time});
            super::replay::kimi_code_record(&v, &mut state, opts, row.sources, result)?;
        }
    }
    Ok(())
}
