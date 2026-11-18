//! Per-record classification of the Kimi Code v2 wire journal. Context records
//! arrive here after `kimi_v2::fold`; everything else is the vendor's
//! "bookkeeping" (packages/agent-core-v2/src/agent/fullCompaction/
//! context-recovery-footer.md): kept as native Content when it can carry text,
//! otherwise recorded as ignored.
use super::*;
use crate::parser::{required, string};

fn messages(value: &Value) -> Result<&Vec<Value>, LineErrorKind> {
    value
        .as_array()
        .ok_or_else(|| LineErrorKind::InvalidField("messages".into()))
}

pub(super) fn record(
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
        | "context.undo"
        // Identity, timing and context-size estimates only; no text and no
        // billed usage (token_counting is the measured context window).
        | "agent.turn.started"
        | "runtime.set_binding"
        | "token_counting.measured"
        | "token_counting.truncated"
        | "token_counting.rebased"
        | "token_counting.turn_recorded"
        | "prompt.completed"
        | "prompt.aborted"
        | "file_history.tracked"
        | "file_history.checkpoint" => p.ignored.push(format!("kimi:v2:{kind}")),
        // The agent-loop engine's own history mirrors context.* messages, and
        // its meta.usage mirrors usage.record: keep it native, never project it
        // a second time as Message/ToolCall/ToolResult/Usage.
        "agent.message.appended" => {
            let role = v
                .pointer("/message/message/role")
                .and_then(Value::as_str)
                .and_then(Role::parse);
            crate::adapters::native_content(role, kind, v, 1, &mut p);
        }
        // Text-bearing control records outside the folded context: system
        // prompt, steer input (already a context message), interruption and
        // error messages, approvals, background tasks, plugin reminders, tool
        // descriptions. None change the native context fold.
        "agent.turn.ended"
        | "profile.bind"
        | "plugin.session_start"
        | "changeNotifier.snapshot"
        | "turn.steer"
        | "turn.step.interrupted"
        | "permission.record_approval_result"
        | "interaction.request"
        | "interaction.resolved"
        | "task.started"
        | "task.terminated"
        | "mcp.tools_discovered" => crate::adapters::native_content(None, kind, v, 1, &mut p),
        other => p.unknown.push(format!("kimi:v2:{other}")),
    }
    append(p, state, sources, result);
    Ok(())
}
