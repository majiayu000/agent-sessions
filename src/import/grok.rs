//! Grok Build ACP journal and explicit native chat_history snapshots.
use super::*;
use crate::{
    adapters,
    parser::{required, string},
};
use std::collections::HashMap;

struct Row {
    native: Value,
    update: Value,
    sources: Vec<ImportSource>,
}

pub(super) fn import(
    records: &[RawRecord],
    opts: &ReadOptions,
    result: &mut SessionImport,
) -> Result<(), ImportError> {
    let mut rows: Vec<Row> = Vec::new();
    let mut calls = HashMap::<String, usize>::new();
    let mut outputs = HashMap::<String, usize>::new();
    let mut format = None;
    let mut chat_state = State::default();
    for record in records {
        let Some(v) = decode_record(record, opts, result)? else {
            continue;
        };
        let chat = v.get("method").is_none()
            && matches!(
                v.get("type").and_then(Value::as_str),
                Some("system" | "user" | "assistant" | "tool_result" | "reasoning")
            );
        if format.is_some_and(|old| old != chat) {
            return Err(
                LineErrorKind::InvalidField("mixed Grok journal and chat snapshot".into()).into(),
            );
        }
        format = Some(chat);
        if chat {
            let mut p = Parsed {
                include: opts.include,
                record_id: string(&v, "id"),
                ..Default::default()
            };
            let mut message = v.clone();
            message["role"] = Value::String(if v["type"] == "tool_result" {
                "tool".into()
            } else {
                required(&v, "type")?.into()
            });
            if v["type"] == "reasoning" {
                adapters::native_content(Some(Role::Assistant), "reasoning", &v, 1, &mut p);
            } else {
                adapters::api_message(&message, &mut p)?;
            }
            if let Some(model) = string(&v, "model_id") {
                chat_state.model = Some(model.clone());
                p.emit(
                    0,
                    Event::Meta(MetaUpdate {
                        model: Some(model),
                        ..Default::default()
                    }),
                );
            }
            append(p, &chat_state, vec![record_source(record)], result);
            continue;
        }
        if !matches!(
            v.get("method").and_then(Value::as_str),
            Some("session/update" | "_x.ai/session/update")
        ) {
            tally(&mut result.summary.unknown_types, "grok:method");
            continue;
        }
        let update = v
            .pointer("/params/update")
            .ok_or(LineErrorKind::MissingField("params.update"))?;
        let ty = required(update, "sessionUpdate")?;
        let source = record_source(record);
        if matches!(ty, "tool_call" | "tool_call_update") {
            if !opts.include.contains(EventKinds::TOOL_CALL)
                && !opts.include.contains(EventKinds::TOOL_RESULT)
            {
                continue;
            }
            let id = required(update, "toolCallId")?.to_owned();
            let index = if let Some(index) = calls.get(&id) {
                *index
            } else {
                let index = rows.len();
                calls.insert(id.clone(), index);
                rows.push(Row {
                    native: v.clone(),
                    update: serde_json::json!({"sessionUpdate":"tool_call", "toolCallId":id}),
                    sources: Vec::new(),
                });
                index
            };
            let row = &mut rows[index];
            for key in ["rawInput", "_meta"] {
                if let Some(value) = update.get(key) {
                    row.update[key] = value.clone();
                }
            }
            row.sources.push(source.clone());
            if matches!(
                update.get("status").and_then(Value::as_str),
                Some("completed" | "failed")
            ) {
                if let Some(index) = outputs.get(&id) {
                    rows[*index].native = v.clone();
                    rows[*index].update = update.clone();
                    rows[*index].sources.push(source);
                } else {
                    outputs.insert(id, rows.len());
                    rows.push(Row {
                        native: v.clone(),
                        update: update.clone(),
                        sources: vec![source],
                    });
                }
            }
        } else {
            rows.push(Row {
                native: v.clone(),
                update: update.clone(),
                sources: vec![source],
            });
        }
    }
    let mut state = State::default();
    for row in rows {
        let mut p = Parsed {
            include: opts.include,
            accounting: opts.accounting,
            ..Default::default()
        };
        p.at = if let Some(ms) = row.native.pointer("/params/_meta/agentTimestampMs") {
            adapters::millis(Some(ms))?
        } else {
            adapters::seconds(row.native.get("timestamp"))?
        };
        p.record_id = row
            .native
            .pointer("/params/_meta/eventId")
            .and_then(Value::as_str)
            .map(str::to_owned);
        state.session_id = row
            .native
            .pointer("/params/sessionId")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let u = &row.update;
        match required(u, "sessionUpdate")? {
            "user_message_chunk" | "agent_message_chunk" => {
                if p.wants(EventKinds::MESSAGE) || p.wants(EventKinds::CONTENT) {
                    let content = u
                        .get("content")
                        .ok_or(LineErrorKind::MissingField("content"))?;
                    let role = if u["sessionUpdate"] == "user_message_chunk" {
                        Role::User
                    } else {
                        Role::Assistant
                    };
                    adapters::message(role, &Value::Array(vec![content.clone()]), u, &mut p)?;
                }
            }
            "tool_call" => {
                if p.wants(EventKinds::TOOL_CALL) {
                    if let Some(name) = u.pointer("/_meta/x.ai~1tool/name").and_then(Value::as_str)
                    {
                        adapters::call(string(u, "toolCallId"), name, u.get("rawInput"), 0, &mut p);
                    } else {
                        p.unknown.push("grok:tool-without-native-name".into());
                    }
                }
            }
            "tool_call_update" => {
                if p.wants(EventKinds::TOOL_RESULT) {
                    let output = u.get("rawOutput").or_else(|| u.get("content"));
                    if let Some(output) = output {
                        let projection;
                        let text_output = if u.get("rawOutput").is_none() {
                            let content = output
                                .as_array()
                                .ok_or(LineErrorKind::InvalidField("tool.content".into()))?;
                            projection = Value::Array(
                                content
                                    .iter()
                                    .map(|part| {
                                        if part.get("type").and_then(Value::as_str)
                                            == Some("content")
                                        {
                                            part.get("content").cloned().ok_or(
                                                LineErrorKind::MissingField("tool.content.content"),
                                            )
                                        } else {
                                            Ok(part.clone())
                                        }
                                    })
                                    .collect::<Result<Vec<_>, _>>()?,
                            );
                            &projection
                        } else {
                            output
                        };
                        adapters::result(
                            string(u, "toolCallId"),
                            text_output,
                            Some(u["status"] == "failed"),
                            0,
                            &mut p,
                        )?;
                        for (_, event) in &mut p.events {
                            if let Event::ToolResult(r) = event {
                                r.output = Some(output.clone());
                            }
                        }
                        // Native structured output remains intact even when no text projection exists.
                        if let Some(text) = output
                            .pointer("/FileContent/content")
                            .or_else(|| output.get("output_for_prompt"))
                            .or_else(|| output.pointer("/Content/content"))
                            .and_then(Value::as_str)
                        {
                            for (_, event) in &mut p.events {
                                if let Event::ToolResult(r) = event {
                                    r.text = text.into();
                                }
                            }
                        }
                    } else {
                        p.unknown.push("grok:completed-tool-without-output".into());
                    }
                }
            }
            "turn_completed" => {
                if p.wants(EventKinds::USAGE)
                    && let Some(u) = u.get("usage")
                {
                    p.emit(
                        0,
                        Event::Usage(Usage {
                            counts: TokenCounts {
                                input: adapters::counter(u, "inputTokens")?,
                                output: adapters::counter(u, "outputTokens")?,
                                cache_read: adapters::counter(u, "cachedReadTokens")?,
                                cache_write: adapters::counter(u, "cacheCreationTokens")?,
                                reasoning: adapters::counter(u, "reasoningTokens")?,
                                reported_total: adapters::counter(u, "totalTokens")?,
                                ..Default::default()
                            },
                            semantics: TokenSemantics::Unknown,
                            model: None,
                            dedup_key: string(&row.update, "prompt_id"),
                            cumulative: None,
                            basis: UsageBasis::Response,
                            stop_reason: string(&row.update, "stop_reason"),
                            endpoint: Endpoint::Unknown,
                            inference_geo: None,
                            adjustments: Vec::new(),
                        }),
                    );
                }
            }
            "agent_thought_chunk" | "plan" | "session_recap" => {
                adapters::native_content(
                    Some(Role::Assistant),
                    required(u, "sessionUpdate")?,
                    u,
                    1,
                    &mut p,
                );
            }
            "hook_execution"
            | "hook_annotation"
            | "retry_state"
            | "background_tasks"
            | "task_backgrounded"
            | "task_completed"
            | "subagent_spawned"
            | "subagent_finished"
            | "memory_dream_queued"
            | "memory_dream_started"
            | "memory_dream_completed" => {
                p.ignored
                    .push(format!("grok:{}", required(u, "sessionUpdate")?));
            }
            ty => p.unknown.push(format!("grok:{ty}")),
        }
        append(p, &state, row.sources, result);
    }
    Ok(())
}
