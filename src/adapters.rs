//! Native record adapters. Format evidence and exclusions live in docs/support.md.
use crate::parser::{Parsed, State, required, string};
use crate::*;
use chrono::{DateTime, Utc};
use serde_json::Value;

pub(crate) fn seconds(v: Option<&Value>) -> Result<Option<DateTime<Utc>>, LineErrorKind> {
    let Some(v) = v.filter(|v| !v.is_null()) else {
        return Ok(None);
    };
    let seconds = v
        .as_f64()
        .ok_or_else(|| LineErrorKind::InvalidField("timestamp".into()))?;
    let whole = seconds.floor();
    if !seconds.is_finite() || whole < i64::MIN as f64 || whole >= i64::MAX as f64 {
        return Err(LineErrorKind::InvalidField("timestamp".into()));
    }
    DateTime::from_timestamp(whole as i64, ((seconds - whole) * 1e9) as u32)
        .map(Some)
        .ok_or_else(|| LineErrorKind::InvalidField("timestamp".into()))
}
pub(crate) fn millis(v: Option<&Value>) -> Result<Option<DateTime<Utc>>, LineErrorKind> {
    match v.filter(|v| !v.is_null()) {
        None => Ok(None),
        Some(v) => v
            .as_i64()
            .and_then(DateTime::from_timestamp_millis)
            .map(Some)
            .ok_or_else(|| LineErrorKind::InvalidField("timestamp".into())),
    }
}
pub(crate) fn counter(v: &Value, key: &str) -> Result<Option<u64>, LineErrorKind> {
    match v.get(key).filter(|v| !v.is_null()) {
        None => Ok(None),
        Some(v) => v
            .as_u64()
            .map(Some)
            .ok_or_else(|| LineErrorKind::InvalidField(key.into())),
    }
}
pub(crate) fn native_content(
    role: Option<Role>,
    kind: &str,
    data: &Value,
    slot: usize,
    p: &mut Parsed,
) {
    if p.wants(EventKinds::CONTENT) {
        p.emit(
            slot,
            Event::Content(Content {
                role,
                kind: kind.into(),
                data: data.clone(),
            }),
        );
    } else {
        p.ignored.push(format!("content:{kind}"));
    }
}
pub(crate) fn parts(
    parts: &Value,
    p: &mut Parsed,
    role: Option<Role>,
) -> Result<(String, Vec<std::ops::Range<usize>>), LineErrorKind> {
    if let Some(text) = parts.as_str() {
        return Ok((text.into(), std::iter::once(0..text.len()).collect()));
    }
    if parts.is_null() {
        return Ok((String::new(), Vec::new()));
    }
    let blocks = parts
        .as_array()
        .ok_or_else(|| LineErrorKind::InvalidField("content".into()))?;
    let mut body = String::new();
    let mut segments = Vec::new();
    for (i, b) in blocks.iter().enumerate() {
        if let Some(text) = b.as_str() {
            if !segments.is_empty() {
                body.push('\n');
            }
            let start = body.len();
            body.push_str(text);
            segments.push(start..body.len());
            continue;
        }
        let ty = b.get("type").and_then(Value::as_str);
        if b.get("thought").and_then(Value::as_bool) == Some(true) {
            if role.is_some() {
                native_content(role, "thinking", b, 3 + i, p);
            }
            continue;
        }
        let text = match ty {
            Some("text" | "input_text" | "output_text" | "Text" | "inputText") => {
                Some(required(b, "text")?)
            }
            None if b.get("text").is_some() => Some(required(b, "text")?),
            Some(
                "tool_use" | "server_tool_use" | "tool_result" | "toolCall" | "toolRequest"
                | "toolResponse",
            ) => None,
            None if b.get("functionCall").is_some() || b.get("functionResponse").is_some() => None,
            Some(
                "thinking" | "think" | "reasoning" | "redacted_thinking" | "redactedThinking"
                | "redacted-reasoning" | "image" | "imageUrl" | "file" | "input_image"
                | "output_image" | "image_url" | "audio_url" | "video_url" | "inputImage"
                | "inputAudio" | "document" | "image_blob_ref" | "audio" | "video" | "input_audio"
                | "output_audio" | "resource" | "resource_link" | "tool_reference",
            ) => {
                if role.is_some() {
                    native_content(role, ty.unwrap(), b, 3 + i, p);
                }
                None
            }
            None if b.get("inlineData").is_some()
                || b.get("fileData").is_some()
                || b.get("thoughtSignature").is_some() =>
            {
                if role.is_some() {
                    native_content(role, "media-or-signature", b, 3 + i, p);
                }
                None
            }
            Some(other) => {
                p.unknown.push(format!("content:{other}"));
                None
            }
            None => {
                p.unknown.push("content:untyped".into());
                None
            }
        };
        if let Some(text) = text {
            if role.is_some()
                && b.as_object()
                    .is_some_and(|v| v.keys().any(|k| !matches!(k.as_str(), "type" | "text")))
            {
                native_content(role, ty.unwrap_or("text"), b, 3 + i, p);
            }
            if !segments.is_empty() {
                body.push('\n');
            }
            let start = body.len();
            body.push_str(text);
            segments.push(start..body.len());
        }
    }
    Ok((body, segments))
}
pub(crate) fn message(
    role: Role,
    content: &Value,
    native: &Value,
    p: &mut Parsed,
) -> Result<(), LineErrorKind> {
    if !p.wants(EventKinds::MESSAGE) && !p.wants(EventKinds::CONTENT) {
        return Ok(());
    }
    let (text, text_segments) = parts(content, p, Some(role))?;
    p.emit(
        1,
        Event::Message(Message {
            role,
            text,
            text_segments,
            is_meta: crate::parser::flag(
                native.get("isMeta").or_else(|| native.get("is_meta")),
                "isMeta",
            )?,
            is_sidechain: crate::parser::flag(native.get("isSidechain"), "isSidechain")?,
            parent_id: string(native, "parentUuid").or_else(|| string(native, "parentId")),
        }),
    );
    Ok(())
}
pub(crate) fn call(
    id: Option<String>,
    name: &str,
    args: Option<&Value>,
    slot: usize,
    p: &mut Parsed,
) {
    if p.wants(EventKinds::TOOL_CALL) {
        p.emit(
            slot,
            Event::ToolCall(ToolCall {
                kind: ToolCallKind::Function,
                id,
                name: name.into(),
                arguments: match args {
                    None | Some(Value::Null) => ToolArgs::Missing,
                    Some(Value::String(s)) => ToolArgs::RawString(s.clone()),
                    Some(v) => ToolArgs::Json(v.clone()),
                },
            }),
        );
    }
}
pub(crate) fn result(
    id: Option<String>,
    output: &Value,
    error: Option<bool>,
    slot: usize,
    p: &mut Parsed,
) -> Result<(), LineErrorKind> {
    if !p.wants(EventKinds::TOOL_RESULT) {
        return Ok(());
    }
    let text = if output.is_string() || output.is_array() || output.is_null() {
        parts(output, p, None)?.0
    } else {
        output
            .get("text")
            .or_else(|| output.get("content"))
            .filter(|v| v.is_string() || v.is_array())
            .map(|v| parts(v, p, None))
            .transpose()?
            .map(|v| v.0)
            .unwrap_or_default()
    };
    p.emit(
        slot,
        Event::ToolResult(ToolResult {
            call_id: id,
            is_error: error,
            text,
            output: Some(output.clone()),
        }),
    );
    Ok(())
}
fn usage(
    counts: TokenCounts,
    semantics: TokenSemantics,
    model: Option<String>,
    key: Option<String>,
    p: &mut Parsed,
) {
    p.emit(
        2,
        Event::Usage(Usage {
            counts,
            semantics,
            model,
            dedup_key: key,
            cumulative: None,
            basis: UsageBasis::Message,
            stop_reason: None,
            endpoint: Endpoint::Unknown,
            inference_geo: None,
            adjustments: Vec::new(),
        }),
    );
}
pub(crate) fn gemini_usage(
    v: &Value,
    normalized: bool,
    model: Option<String>,
    key: Option<String>,
    p: &mut Parsed,
) -> Result<(), LineErrorKind> {
    if !p.wants(EventKinds::USAGE) || v.is_null() {
        return Ok(());
    }
    if !v.is_object() {
        return Err(LineErrorKind::InvalidField("usage".into()));
    }
    usage(
        TokenCounts {
            input: counter(
                v,
                if normalized {
                    "input"
                } else {
                    "promptTokenCount"
                },
            )?,
            output: counter(
                v,
                if normalized {
                    "output"
                } else {
                    "candidatesTokenCount"
                },
            )?,
            cache_read: counter(
                v,
                if normalized {
                    "cached"
                } else {
                    "cachedContentTokenCount"
                },
            )?,
            reasoning: counter(
                v,
                if normalized {
                    "thoughts"
                } else {
                    "thoughtsTokenCount"
                },
            )?,
            reported_total: counter(
                v,
                if normalized {
                    "total"
                } else {
                    "totalTokenCount"
                },
            )?,
            ..Default::default()
        },
        TokenSemantics::Gemini,
        model,
        key,
        p,
    );
    Ok(())
}
fn api_tools(content: &Value, p: &mut Parsed) -> Result<(), LineErrorKind> {
    if !p.wants(EventKinds::TOOL_CALL) && !p.wants(EventKinds::TOOL_RESULT) {
        return Ok(());
    }
    let Some(blocks) = content.as_array() else {
        return Ok(());
    };
    for (i, b) in blocks.iter().enumerate() {
        match b.get("type").and_then(Value::as_str) {
            Some("tool_use" | "server_tool_use" | "toolCall") if p.wants(EventKinds::TOOL_CALL) => {
                call(
                    string(b, "id"),
                    required(b, "name")?,
                    b.get("input").or_else(|| b.get("arguments")),
                    3 + i,
                    p,
                );
            }
            Some("tool_result") if p.wants(EventKinds::TOOL_RESULT) => result(
                string(b, "tool_use_id"),
                b.get("content")
                    .ok_or(LineErrorKind::MissingField("tool_result.content"))?,
                b.get("is_error").and_then(Value::as_bool),
                3 + i,
                p,
            )?,
            _ => {}
        }
        if p.wants(EventKinds::TOOL_CALL)
            && let Some(c) = b.get("functionCall")
        {
            call(
                string(c, "id"),
                required(c, "name")?,
                c.get("args"),
                3 + i,
                p,
            );
        }
        if p.wants(EventKinds::TOOL_RESULT)
            && let Some(r) = b.get("functionResponse")
        {
            result(
                string(r, "id"),
                r.get("response")
                    .ok_or(LineErrorKind::MissingField("functionResponse.response"))?,
                None,
                3 + i,
                p,
            )?;
        }
    }
    Ok(())
}
pub(crate) fn api_message(v: &Value, p: &mut Parsed) -> Result<(), LineErrorKind> {
    if !p.wants(EventKinds::MESSAGE)
        && !p.wants(EventKinds::CONTENT)
        && !p.wants(EventKinds::TOOL_CALL)
        && !p.wants(EventKinds::TOOL_RESULT)
    {
        return Ok(());
    }
    if v.get("type").and_then(Value::as_str) == Some("reasoning")
        || v.get("role").and_then(Value::as_str) == Some("thinking")
    {
        native_content(Some(Role::Assistant), "reasoning", v, 1, p);
        return Ok(());
    }
    let role = required(v, "role")?;
    let content = v
        .get("content")
        .ok_or(LineErrorKind::MissingField("content"))?;
    if matches!(role, "tool" | "toolResult") {
        return result(
            string(v, "tool_call_id").or_else(|| string(v, "toolCallId")),
            content,
            v.get("isError").and_then(Value::as_bool),
            3,
            p,
        );
    }
    if let Some(role) = Role::parse(role) {
        message(role, content, v, p)?;
    } else {
        p.unknown.push(format!("role:{role}"));
    }
    api_tools(content, p)?;
    if p.wants(EventKinds::TOOL_CALL)
        && let Some(calls) = v.get("tool_calls").or_else(|| v.get("toolCalls"))
    {
        for (i, c) in calls
            .as_array()
            .ok_or_else(|| LineErrorKind::InvalidField("tool_calls".into()))?
            .iter()
            .enumerate()
        {
            let f = c
                .get("function")
                .ok_or(LineErrorKind::MissingField("function"))?;
            call(
                string(c, "id"),
                required(f, "name")?,
                f.get("arguments"),
                3 + content.as_array().map_or(0, Vec::len) + i,
                p,
            );
        }
    }
    for (i, key) in [
        "reasoning",
        "reasoning_content",
        "reasoning_details",
        "codex_reasoning_items",
        "codex_message_items",
    ]
    .into_iter()
    .enumerate()
    {
        if let Some(data) = v.get(key).filter(|v| !v.is_null()) {
            let slot = 3
                + content.as_array().map_or(0, Vec::len)
                + v.get("tool_calls")
                    .or_else(|| v.get("toolCalls"))
                    .and_then(Value::as_array)
                    .map_or(0, Vec::len)
                + i;
            native_content(Role::parse(role), key, data, slot, p);
        }
    }
    Ok(())
}
pub(crate) fn parse(
    agent: Agent,
    v: &Value,
    state: &mut State,
    p: &mut Parsed,
) -> Result<(), LineErrorKind> {
    match agent {
        Agent::QwenCode => qwen(v, state, p),
        Agent::Pi => pi(v, state, p),
        Agent::KimiCli => kimi_context(v, p),
        Agent::CopilotCli => copilot(v, state, p),
        Agent::GeminiCli => gemini(v, state, p),
        Agent::Cline | Agent::RooCode => api_message(v, p),
        Agent::Hermes => api_message(v, p),
        Agent::ClineCli => {
            p.message_id = string(v, "id");
            // CLI saves internal tool results as {query,result,success} entries,
            // rather than Anthropic text blocks. Retain that native array intact.
            let mut v = v.clone();
            if p.wants(EventKinds::TOOL_RESULT)
                && let Some(blocks) = v.get_mut("content").and_then(Value::as_array_mut)
            {
                let mut retained = Vec::new();
                for (i, block) in blocks.drain(..).enumerate() {
                    let structured = block.get("type").and_then(Value::as_str)
                        == Some("tool_result")
                        && block
                            .get("content")
                            .and_then(Value::as_array)
                            .is_some_and(|rows| {
                                !rows.is_empty()
                                    && rows.iter().all(|r| {
                                        r.get("success").and_then(Value::as_bool).is_some()
                                            && r.get("result").is_some()
                                    })
                            });
                    if structured {
                        let output = &block["content"];
                        let text = output
                            .as_array()
                            .unwrap()
                            .iter()
                            .filter_map(|r| r.get("result").and_then(Value::as_str))
                            .collect::<Vec<_>>()
                            .join("\n");
                        p.emit(
                            3 + i,
                            Event::ToolResult(ToolResult {
                                call_id: string(&block, "tool_use_id"),
                                is_error: block.get("is_error").and_then(Value::as_bool),
                                text,
                                output: Some(output.clone()),
                            }),
                        );
                    } else {
                        retained.push(block);
                    }
                }
                *blocks = retained;
            }
            api_message(&v, p)?;
            if p.wants(EventKinds::USAGE)
                && let Some(metrics) = v.get("metrics")
            {
                usage(
                    TokenCounts {
                        input: counter(metrics, "inputTokens")?,
                        output: counter(metrics, "outputTokens")?,
                        cache_read: counter(metrics, "cacheReadTokens")?,
                        cache_write: counter(metrics, "cacheWriteTokens")?,
                        ..Default::default()
                    },
                    TokenSemantics::Unknown,
                    None,
                    p.message_id.clone(),
                    p,
                );
            }
            Ok(())
        }
        Agent::OpenCode => opencode(v, state, p),
        Agent::ZCode => {
            let mut native = v.clone();
            if let Some(parts) = native.get_mut("parts").and_then(Value::as_array_mut) {
                parts.retain(|part| {
                    if part.get("type").and_then(Value::as_str) == Some("timeline") {
                        p.ignored.push("zcode:model-timeline".into());
                        false
                    } else {
                        true
                    }
                });
            }
            opencode(&native, state, p)
        }
        Agent::Zed => zed(v, p),
        Agent::Antigravity => antigravity(v, p),
        Agent::WorkBuddy => workbuddy(v, state, p),
        Agent::Qoder => qoder(v, state, p),
        Agent::GrokBot => grokbot(v, p),
        Agent::Continue => {
            let m = v
                .get("message")
                .ok_or(LineErrorKind::MissingField("message"))?;
            api_message(m, p)?;
            if let Some(context) = v.get("contextItems") {
                let slot = p
                    .events
                    .iter()
                    .map(|(slot, _)| *slot)
                    .max()
                    .unwrap_or(2)
                    .max(2)
                    + 1;
                native_content(None, "contextItems", context, slot, p);
            }
            if p.wants(EventKinds::USAGE)
                && let Some(u) = m.get("usage")
            {
                usage(
                    TokenCounts {
                        input: counter(u, "promptTokens")?,
                        output: counter(u, "completionTokens")?,
                        cache_read: counter(
                            u.get("promptTokensDetails").unwrap_or(&Value::Null),
                            "cachedTokens",
                        )?,
                        cache_write: counter(
                            u.get("promptTokensDetails").unwrap_or(&Value::Null),
                            "cacheWriteTokens",
                        )?,
                        reasoning: counter(
                            u.get("completionTokensDetails").unwrap_or(&Value::Null),
                            "reasoningTokens",
                        )?,
                        ..Default::default()
                    },
                    TokenSemantics::Unknown,
                    None,
                    None,
                    p,
                );
            }
            Ok(())
        }
        Agent::Goose => goose(v, p),
        Agent::Cursor => cursor(v, p),
        Agent::CodeBuddy => codebuddy(v, state, p),
        Agent::IFlow => anthropic_session(v, state, p),
        _ => Err(LineErrorKind::InvalidField(
            "unsupported record source".into(),
        )),
    }
}
// CodeBuddy's native SDK history differs from its Claude stream-json view.
fn codebuddy(v: &Value, state: &mut State, p: &mut Parsed) -> Result<(), LineErrorKind> {
    if let Some(payload) = v.get("payload").filter(|v| v.is_object()) {
        return codebuddy(payload, state, p);
    }
    let kind = required(v, "type")?;
    match kind {
        "message" | "function_call" | "function_call_result" | "reasoning" => {
            workbuddy(v, state, p)?
        }
        "function_call_output" => result(
            string(v, "callId"),
            v.get("output")
                .ok_or(LineErrorKind::MissingField("output"))?,
            None,
            3,
            p,
        )?,
        "session-meta"
        | "custom-title"
        | "ai-title"
        | "topic"
        | "summary"
        | "goal-result"
        | "goal-progress"
        | "turn-metrics"
        | "resend-fork-notice"
        | "acp-terminal-state"
        | "model-usage"
        | "credit-usage"
        | "file-history-snapshot" => {
            p.record_id = string(v, "id");
            native_content(None, kind, v, 1, p);
        }
        _ => return anthropic_session(v, state, p),
    }
    if p.wants(EventKinds::USAGE)
        && (kind == "model-usage" || kind == "message" && v["role"] == "assistant")
        && let Some(u) = v.pointer("/providerData/usage")
    {
        usage(
            TokenCounts {
                input: counter(u, "inputTokens")?,
                output: counter(u, "outputTokens")?,
                reported_total: counter(u, "totalTokens")?,
                ..Default::default()
            },
            TokenSemantics::Unknown,
            state.model.clone(),
            v.pointer("/providerData/messageId")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .or_else(|| string(v, "id")),
            p,
        );
    }
    Ok(())
}

fn anthropic_session(v: &Value, state: &mut State, p: &mut Parsed) -> Result<(), LineErrorKind> {
    let kind = required(v, "type")?;
    if !matches!(kind, "user" | "assistant") {
        return crate::claude::parse(v, state, p);
    }
    if let Some(id) = string(v, "sessionId").or_else(|| string(v, "session_id")) {
        state.session_id = Some(id);
    }
    let m = v
        .get("message")
        .ok_or(LineErrorKind::MissingField("message"))?;
    if let Some(model) = string(m, "model") {
        state.model = Some(model);
    }
    p.record_id = string(v, "uuid");
    p.message_id = string(m, "id");
    p.emit(
        0,
        Event::Meta(MetaUpdate {
            session_id: state.session_id.clone(),
            model: state.model.clone(),
            cwd: string(v, "cwd"),
            git_branch: string(v, "gitBranch"),
            agent_version: string(v, "version"),
            ..Default::default()
        }),
    );
    if p.wants(EventKinds::MESSAGE)
        || p.wants(EventKinds::CONTENT)
        || p.wants(EventKinds::TOOL_CALL)
        || p.wants(EventKinds::TOOL_RESULT)
    {
        let content = m
            .get("content")
            .ok_or(LineErrorKind::MissingField("content"))?;
        message(
            if kind == "user" {
                Role::User
            } else {
                Role::Assistant
            },
            content,
            v,
            p,
        )?;
        api_tools(content, p)?;
    }
    if p.wants(EventKinds::USAGE)
        && let Some(u) = m.get("usage").filter(|v| !v.is_null())
    {
        usage(
            crate::tokens::parse_counts(u, true, false)?,
            TokenSemantics::ClaudeExclusive,
            state.model.clone(),
            p.message_id.clone(),
            p,
        );
        if let Some((_, Event::Usage(u))) = p
            .events
            .iter_mut()
            .find(|(_, e)| matches!(e, Event::Usage(_)))
        {
            u.stop_reason = string(m, "stop_reason");
        }
    }
    Ok(())
}
fn qwen(v: &Value, state: &mut State, p: &mut Parsed) -> Result<(), LineErrorKind> {
    let kind = required(v, "type")?;
    if let Some(id) = string(v, "sessionId") {
        state.session_id = Some(id);
    }
    if let Some(model) = string(v, "model") {
        state.model = Some(model);
    }
    p.message_id = string(v, "uuid");
    p.emit(
        0,
        Event::Meta(MetaUpdate {
            session_id: state.session_id.clone(),
            model: state.model.clone(),
            cwd: string(v, "cwd"),
            git_branch: string(v, "gitBranch"),
            agent_version: string(v, "version"),
            ..Default::default()
        }),
    );
    match kind {
        "user" | "assistant" | "tool_result" => {
            let m = v
                .get("message")
                .ok_or(LineErrorKind::MissingField("message"))?;
            if p.wants(EventKinds::MESSAGE)
                || p.wants(EventKinds::CONTENT)
                || p.wants(EventKinds::TOOL_CALL)
                || p.wants(EventKinds::TOOL_RESULT)
            {
                let parts = m.get("parts").ok_or(LineErrorKind::MissingField("parts"))?;
                if kind != "tool_result" {
                    message(
                        if kind == "user" {
                            Role::User
                        } else {
                            Role::Assistant
                        },
                        parts,
                        v,
                        p,
                    )?;
                }
                api_tools(parts, p)?;
            }
            if let Some(u) = v.get("usageMetadata") {
                gemini_usage(u, false, state.model.clone(), p.message_id.clone(), p)?;
            }
        }
        "system" => {
            if let Some(c) = v
                .pointer("/systemPayload/content")
                .or_else(|| v.pointer("/systemPayload/displayText"))
            {
                message(Role::System, c, v, p)?;
            }
            if let Some(payload) = v.get("systemPayload") {
                native_content(Some(Role::System), "systemPayload", payload, 3, p);
            }
            p.ignored.push(format!(
                "system:{}",
                v.get("subtype")
                    .and_then(Value::as_str)
                    .unwrap_or("unspecified")
            ));
        }
        other => p.unknown.push(other.into()),
    }
    Ok(())
}
fn pi(v: &Value, state: &mut State, p: &mut Parsed) -> Result<(), LineErrorKind> {
    p.record_id = string(v, "id");
    p.message_id = string(v, "id");
    match required(v, "type")? {
        "session" => {
            state.session_id = string(v, "id");
            p.emit(
                0,
                Event::Meta(MetaUpdate {
                    session_id: state.session_id.clone(),
                    cwd: string(v, "cwd"),
                    ..Default::default()
                }),
            );
        }
        "model_change" => {
            state.model = string(v, "modelId");
            p.emit(
                0,
                Event::Meta(MetaUpdate {
                    model: state.model.clone(),
                    ..Default::default()
                }),
            );
        }
        "message" => {
            let m = v
                .get("message")
                .ok_or(LineErrorKind::MissingField("message"))?;
            if p.at.is_none() {
                p.at = millis(m.get("timestamp"))?;
            }
            match m.get("role").and_then(Value::as_str) {
                Some("bashExecution") => {
                    native_content(Some(Role::User), "bashExecution", m, 1, p);
                    call(None, "bash", m.get("command"), 3, p);
                    if let Some(output) = m.get("output") {
                        result(
                            None,
                            output,
                            m.get("exitCode")
                                .and_then(Value::as_i64)
                                .map(|code| code != 0)
                                .or_else(|| {
                                    m.get("cancelled").and_then(Value::as_bool).filter(|v| *v)
                                }),
                            4,
                            p,
                        )?;
                    }
                }
                Some("custom") => {
                    native_content(
                        Some(Role::User),
                        "custom",
                        m,
                        3 + m
                            .get("content")
                            .and_then(Value::as_array)
                            .map_or(0, Vec::len),
                        p,
                    );
                    message(
                        Role::User,
                        m.get("content")
                            .ok_or(LineErrorKind::MissingField("content"))?,
                        m,
                        p,
                    )?;
                }
                Some(kind @ ("branchSummary" | "compactionSummary")) => {
                    native_content(None, kind, m, 3, p);
                    message(
                        Role::System,
                        m.get("summary")
                            .ok_or(LineErrorKind::MissingField("summary"))?,
                        m,
                        p,
                    )?;
                }
                _ => api_message(m, p)?,
            }
            for (_, event) in &mut p.events {
                if let Event::Message(message) = event {
                    message.parent_id = string(v, "parentId");
                }
            }
            if p.wants(EventKinds::USAGE)
                && let Some(u) = m.get("usage")
            {
                pi_usage(
                    u,
                    string(m, "model").or_else(|| state.model.clone()),
                    string(m, "responseId").or_else(|| p.message_id.clone()),
                    p,
                )?;
            }
        }
        "usage" => {
            if p.wants(EventKinds::USAGE) {
                pi_usage(
                    v.get("usage").ok_or(LineErrorKind::MissingField("usage"))?,
                    string(v, "model"),
                    p.message_id.clone(),
                    p,
                )?;
            }
        }
        "compaction" | "branch_summary" => {
            native_content(None, required(v, "type")?, v, 3, p);
            if let Some(summary) = v.get("summary") {
                message(Role::System, summary, v, p)?;
            }
            if p.wants(EventKinds::USAGE)
                && let Some(u) = v.get("usage")
            {
                pi_usage(u, state.model.clone(), p.message_id.clone(), p)?;
            }
        }
        "custom_message" => {
            if let Some(content) = v.get("content") {
                message(Role::User, content, v, p)?;
                native_content(
                    Some(Role::User),
                    "custom_message",
                    v,
                    3 + content.as_array().map_or(0, Vec::len),
                    p,
                );
            }
        }
        "thinking_level_change" | "custom" | "label" | "session_info" | "context_edit" => {
            native_content(None, required(v, "type")?, v, 1, p);
        }
        other => p.unknown.push(other.into()),
    }
    Ok(())
}
fn pi_usage(
    v: &Value,
    model: Option<String>,
    key: Option<String>,
    p: &mut Parsed,
) -> Result<(), LineErrorKind> {
    if !v.is_object() {
        return Err(LineErrorKind::InvalidField("usage".into()));
    }
    let counts = TokenCounts {
        input: counter(v, "input")?,
        output: counter(v, "output")?,
        cache_read: counter(v, "cacheRead")?,
        cache_write: counter(v, "cacheWrite")?,
        cache_write_1h: counter(v, "cacheWrite1h")?,
        reasoning: counter(v, "reasoning")?,
        reported_total: counter(v, "totalTokens")?,
    };
    if matches!((counts.cache_write_1h,counts.cache_write),(Some(a),Some(b)) if a>b) {
        return Err(LineErrorKind::InvalidField("cacheWrite1h".into()));
    }
    usage(counts, TokenSemantics::CacheExclusive, model, key, p);
    Ok(())
}
fn kimi_context(v: &Value, p: &mut Parsed) -> Result<(), LineErrorKind> {
    match required(v, "role")? {
        "_checkpoint" | "_usage" => p.ignored.push(required(v, "role")?.into()),
        "_system_prompt" => message(
            Role::System,
            v.get("content")
                .ok_or(LineErrorKind::MissingField("content"))?,
            v,
            p,
        )?,
        _ => api_message(v, p)?,
    }
    Ok(())
}
fn copilot(v: &Value, state: &mut State, p: &mut Parsed) -> Result<(), LineErrorKind> {
    let kind = required(v, "type")?;
    let d = v.get("data").ok_or(LineErrorKind::MissingField("data"))?;
    p.record_id = string(v, "id");
    p.message_id = string(d, "messageId").or_else(|| string(v, "id"));
    match kind {
        "session.start" | "session.resume" | "session.context_changed" => {
            if let Some(id) = string(d, "sessionId") {
                state.session_id = Some(id);
            }
            if let Some(model) = string(d, "selectedModel").or_else(|| string(d, "model")) {
                state.model = Some(model);
            }
            let context = d.get("context").unwrap_or(d);
            p.emit(
                0,
                Event::Meta(MetaUpdate {
                    session_id: state.session_id.clone(),
                    model: state.model.clone(),
                    cwd: string(context, "cwd"),
                    git_branch: string(context, "branch"),
                    agent_version: string(d, "copilotVersion"),
                    ..Default::default()
                }),
            );
        }
        "user.message" | "assistant.message" | "system.message" => {
            let role = match kind {
                "user.message" => Role::User,
                "assistant.message" => Role::Assistant,
                _ => Role::parse(required(d, "role")?)
                    .ok_or_else(|| LineErrorKind::InvalidField("role".into()))?,
            };
            message(
                role,
                d.get("content")
                    .ok_or(LineErrorKind::MissingField("content"))?,
                v,
                p,
            )?;
            if let Some((_, Event::Message(m))) = p
                .events
                .iter_mut()
                .find(|(_, e)| matches!(e, Event::Message(_)))
            {
                m.is_sidechain = v.get("agentId").is_some_and(|v| !v.is_null());
            }
            for key in ["attachments", "toolRequests"] {
                if let Some(data) = d.get(key) {
                    let slot = p
                        .events
                        .iter()
                        .map(|(slot, _)| *slot)
                        .max()
                        .unwrap_or(2)
                        .max(2)
                        + 1;
                    native_content(Some(role), key, data, slot, p);
                }
            }
            // Tool requests are intent; execution_start is the canonical call record.
        }
        "tool.execution_start" if p.wants(EventKinds::TOOL_CALL) => call(
            string(d, "toolCallId"),
            required(d, "toolName")?,
            d.get("arguments"),
            3,
            p,
        ),
        "tool.execution_complete" if p.wants(EventKinds::TOOL_RESULT) => {
            let success = d
                .get("success")
                .and_then(Value::as_bool)
                .ok_or_else(|| LineErrorKind::InvalidField("success".into()))?;
            let output = d
                .pointer("/result/content")
                .or_else(|| d.pointer("/error/message"))
                .unwrap_or(&Value::Null);
            result(string(d, "toolCallId"), output, Some(!success), 3, p)?;
        }
        "assistant.usage" if p.wants(EventKinds::USAGE) => usage(
            TokenCounts {
                input: counter(d, "inputTokens")?,
                output: counter(d, "outputTokens")?,
                cache_read: counter(d, "cacheReadTokens")?,
                cache_write: counter(d, "cacheWriteTokens")?,
                reasoning: counter(d, "reasoningTokens")?,
                ..Default::default()
            },
            TokenSemantics::Unknown,
            string(d, "model"),
            p.record_id.clone(),
            p,
        ),
        "session.compaction_complete" => {
            if let Some(c) = d.get("summaryContent") {
                message(Role::System, c, v, p)?;
            }
            p.ignored.push(kind.into());
        }
        "session.model_change" => {
            state.model = string(d, "newModel");
            p.emit(
                0,
                Event::Meta(MetaUpdate {
                    model: state.model.clone(),
                    ..Default::default()
                }),
            );
        }
        "assistant.reasoning" | "assistant.reasoning_delta" => {
            native_content(Some(Role::Assistant), kind, d, 1, p)
        }
        "tool.execution_start"
        | "tool.execution_complete"
        | "assistant.usage"
        | "assistant.turn_start"
        | "assistant.turn_end"
        | "assistant.message_delta"
        | "assistant.intent"
        | "assistant.streaming_delta"
        | "tool.execution_partial_result"
        | "tool.execution_progress"
        | "tool.user_requested"
        | "session.idle"
        | "session.error"
        | "session.shutdown"
        | "session.task_complete"
        | "session.compaction_start"
        | "session.title_changed"
        | "session.usage_info"
        | "session.usage_checkpoint"
        | "session.session_limits_changed"
        | "permission.requested"
        | "permission.completed"
        | "subagent.started"
        | "subagent.completed"
        | "subagent.failed"
        | "subagent.selected"
        | "skill.invoked"
        | "abort" => p.ignored.push(kind.into()),
        other => p.unknown.push(other.into()),
    }
    Ok(())
}
fn gemini(v: &Value, state: &mut State, p: &mut Parsed) -> Result<(), LineErrorKind> {
    p.message_id = string(v, "id");
    let kind = required(v, "type")?;
    match kind {
        "user" | "gemini" => {
            if p.wants(EventKinds::MESSAGE) || p.wants(EventKinds::CONTENT) {
                let c = v
                    .get("content")
                    .ok_or(LineErrorKind::MissingField("content"))?;
                message(
                    if kind == "user" {
                        Role::User
                    } else {
                        Role::Assistant
                    },
                    c,
                    v,
                    p,
                )?;
            }
            if let Some(calls) = v.get("toolCalls") {
                for (i, c) in calls
                    .as_array()
                    .ok_or_else(|| LineErrorKind::InvalidField("toolCalls".into()))?
                    .iter()
                    .enumerate()
                {
                    if p.wants(EventKinds::TOOL_CALL) {
                        call(
                            string(c, "id"),
                            required(c, "name")?,
                            c.get("args"),
                            3 + 2 * i,
                            p,
                        );
                    }
                    if let Some(r) = c.get("result").filter(|r| !r.is_null()) {
                        result(
                            string(c, "id"),
                            r,
                            c.get("status")
                                .and_then(Value::as_str)
                                .map(|s| s == "error"),
                            4 + 2 * i,
                            p,
                        )?;
                    }
                }
            }
            if let Some(thoughts) = v.get("thoughts") {
                let slot = p
                    .events
                    .iter()
                    .map(|(slot, _)| *slot)
                    .max()
                    .unwrap_or(2)
                    .max(2)
                    + 1;
                native_content(Some(Role::Assistant), "thoughts", thoughts, slot, p);
            }
            if let Some(u) = v.get("tokens") {
                gemini_usage(
                    u,
                    true,
                    string(v, "model").or_else(|| state.model.clone()),
                    p.message_id.clone(),
                    p,
                )?;
            }
        }
        "info" | "error" | "warning" => message(
            Role::System,
            v.get("content")
                .ok_or(LineErrorKind::MissingField("content"))?,
            v,
            p,
        )?,
        other => p.unknown.push(other.into()),
    }
    Ok(())
}
fn opencode(v: &Value, state: &mut State, p: &mut Parsed) -> Result<(), LineErrorKind> {
    let info = v.get("info").ok_or(LineErrorKind::MissingField("info"))?;
    let role = required(info, "role")?;
    p.message_id = string(info, "id");
    p.record_id = p.message_id.clone();
    p.at = millis(info.pointer("/time/created"))?;
    if let Some(id) = string(info, "sessionID") {
        state.session_id = Some(id);
    }
    if let Some(model) = string(info, "modelID") {
        state.model = Some(model);
    }
    let blocks = v
        .get("parts")
        .and_then(Value::as_array)
        .ok_or(LineErrorKind::MissingField("parts"))?;
    for (i, b) in blocks.iter().enumerate() {
        if let Some(
            kind @ ("step-start" | "step-finish" | "snapshot" | "patch" | "subtask" | "agent"
            | "retry" | "compaction"),
        ) = b.get("type").and_then(Value::as_str)
        {
            if matches!(kind, "patch" | "subtask" | "compaction" | "snapshot") {
                native_content(Role::parse(role), kind, b, 3 + i, p);
            } else {
                p.ignored.push(format!("part:{kind}"));
            }
        }
    }
    let text = Value::Array(
        blocks
            .iter()
            .filter(|b| {
                !matches!(
                    b.get("type").and_then(Value::as_str),
                    Some(
                        "tool"
                            | "step-start"
                            | "step-finish"
                            | "snapshot"
                            | "patch"
                            | "subtask"
                            | "agent"
                            | "retry"
                            | "compaction"
                    )
                )
            })
            .cloned()
            .collect(),
    );
    if let Some(role) = Role::parse(role) {
        message(role, &text, info, p)?;
    } else {
        p.unknown.push(format!("role:{role}"));
    }
    for (i, b) in blocks
        .iter()
        .enumerate()
        .filter(|(_, b)| b.get("type").and_then(Value::as_str) == Some("tool"))
    {
        if !p.wants(EventKinds::TOOL_CALL) && !p.wants(EventKinds::TOOL_RESULT) {
            continue;
        }
        let s = b
            .get("state")
            .ok_or(LineErrorKind::MissingField("tool.state"))?;
        if p.wants(EventKinds::TOOL_CALL) {
            call(
                string(b, "callID"),
                required(b, "tool")?,
                s.get("input"),
                3 + 2 * i,
                p,
            );
        }
        if matches!(
            s.get("status").and_then(Value::as_str),
            Some("completed" | "error")
        ) {
            result(
                string(b, "callID"),
                s.get("output")
                    .or_else(|| s.get("error"))
                    .ok_or(LineErrorKind::MissingField("tool.output"))?,
                Some(s["status"] == "error"),
                4 + 2 * i,
                p,
            )?;
        }
    }
    if p.wants(EventKinds::USAGE)
        && let Some(u) = info.get("tokens")
    {
        usage(
            TokenCounts {
                input: counter(u, "input")?,
                output: counter(u, "output")?,
                reasoning: counter(u, "reasoning")?,
                cache_read: counter(u.get("cache").unwrap_or(&Value::Null), "read")?,
                cache_write: counter(u.get("cache").unwrap_or(&Value::Null), "write")?,
                reported_total: counter(u, "total")?,
                ..Default::default()
            },
            TokenSemantics::Unknown,
            state.model.clone(),
            p.message_id.clone(),
            p,
        );
    }
    Ok(())
}
fn goose(v: &Value, p: &mut Parsed) -> Result<(), LineErrorKind> {
    p.message_id = string(v, "id");
    p.at = seconds(v.get("created"))?;
    let blocks = v
        .get("content")
        .ok_or(LineErrorKind::MissingField("content"))?;
    let role = required(v, "role")?;
    if let Some(role) = Role::parse(role) {
        message(role, blocks, v, p)?;
    } else {
        p.unknown.push(format!("role:{role}"));
    }
    if let Some(blocks) = blocks.as_array() {
        for (i, b) in blocks.iter().enumerate() {
            match b.get("type").and_then(Value::as_str) {
                Some("toolRequest") if p.wants(EventKinds::TOOL_CALL) => {
                    if let Some(c) = b.pointer("/toolCall/value") {
                        call(
                            string(b, "id"),
                            required(c, "name")?,
                            c.get("arguments"),
                            3 + i,
                            p,
                        );
                    } else {
                        p.unknown.push("toolRequest:failed-or-unknown".into());
                    }
                }
                Some("toolResponse") if p.wants(EventKinds::TOOL_RESULT) => {
                    let r = b
                        .get("toolResult")
                        .ok_or(LineErrorKind::MissingField("toolResult"))?;
                    if let Some(output) = r.pointer("/value/content") {
                        result(
                            string(b, "id"),
                            output,
                            r.pointer("/value/isError").and_then(Value::as_bool),
                            3 + i,
                            p,
                        )?;
                    } else if let Some(error) = r.get("error") {
                        result(string(b, "id"), error, Some(true), 3 + i, p)?;
                    } else {
                        p.unknown.push("toolResponse:unknown".into());
                    }
                }
                _ => {}
            }
        }
    }
    if p.wants(EventKinds::USAGE)
        && let Some(u) = v.pointer("/metadata/usage")
    {
        usage(
            TokenCounts {
                input: counter(u, "inputTokens")?,
                output: counter(u, "outputTokens")?,
                cache_read: counter(u, "cacheReadTokens")?,
                cache_write: counter(u, "cacheWriteTokens")?,
                reported_total: counter(u, "totalTokens")?,
                ..Default::default()
            },
            TokenSemantics::Unknown,
            None,
            p.message_id.clone(),
            p,
        );
    }
    Ok(())
}
fn cursor(v: &Value, p: &mut Parsed) -> Result<(), LineErrorKind> {
    p.message_id = string(v, "bubbleId");
    p.at = if v.get("createdAt").is_some_and(Value::is_string) {
        crate::parser::timestamp(v)?
    } else {
        millis(v.get("createdAt"))?
    };
    p.timestamp_text = string(v, "createdAt");
    let role = match v.get("type").and_then(Value::as_u64) {
        Some(1) => Role::User,
        Some(2) => Role::Assistant,
        _ => {
            p.unknown.push("cursor:bubble-type".into());
            return Ok(());
        }
    };
    if let Some(text) = v.get("text").filter(|v| !v.is_null()) {
        message(role, text, v, p)?;
    } else {
        p.unknown.push("cursor:missing-or-encrypted-text".into());
    }
    for (slot, key) in ["thinking", "images", "attachedFiles", "context", "richText"]
        .iter()
        .enumerate()
    {
        if let Some(data) = v.get(key).filter(|v| !v.is_null()) {
            native_content(Some(role), key, data, 5 + slot, p);
        }
    }
    // tokenCount is UI/context bookkeeping, not established per-response usage.
    if let Some(tool) = v.get("toolFormerData").filter(|v| !v.is_null()) {
        if let Some(name) = tool.get("name").and_then(Value::as_str) {
            call(
                string(tool, "toolCallId"),
                name,
                tool.get("rawArgs")
                    .filter(|v| !v.is_null())
                    .or_else(|| tool.get("params")),
                3,
                p,
            );
            if let Some(output) = tool.get("result").filter(|v| !v.is_null()) {
                result(
                    string(tool, "toolCallId"),
                    output,
                    tool.get("status")
                        .and_then(Value::as_str)
                        .map(|s| s == "error"),
                    4,
                    p,
                )?;
            }
        } else {
            p.unknown.push("cursor:tool-payload".into());
        }
    } else if v
        .get("toolResults")
        .and_then(Value::as_array)
        .is_some_and(|v| !v.is_empty())
    {
        p.unknown.push("cursor:tool-payload".into());
    }
    Ok(())
}

fn workbuddy(v: &Value, state: &mut State, p: &mut Parsed) -> Result<(), LineErrorKind> {
    if let Some(id) = string(v, "sessionId") {
        state.session_id = Some(id);
    }
    if let Some(model) = v.pointer("/providerData/model").and_then(Value::as_str) {
        state.model = Some(model.into());
    }
    p.record_id = string(v, "id");
    p.message_id = p.record_id.clone();
    p.emit(
        0,
        Event::Meta(MetaUpdate {
            session_id: state.session_id.clone(),
            model: state.model.clone(),
            cwd: string(v, "cwd"),
            ..Default::default()
        }),
    );
    match required(v, "type")? {
        "message" => {
            if p.wants(EventKinds::MESSAGE) || p.wants(EventKinds::CONTENT) {
                let role = Role::parse(required(v, "role")?)
                    .ok_or_else(|| LineErrorKind::InvalidField("role".into()))?;
                message(
                    role,
                    v.get("content")
                        .ok_or(LineErrorKind::MissingField("content"))?,
                    v,
                    p,
                )?;
            }
        }
        "function_call" if p.wants(EventKinds::TOOL_CALL) => call(
            string(v, "callId"),
            required(v, "name")?,
            v.get("arguments"),
            3,
            p,
        ),
        "function_call_result" if p.wants(EventKinds::TOOL_RESULT) => {
            let error = match v.get("status").and_then(Value::as_str) {
                Some("completed") => Some(false),
                Some("failed" | "error") => Some(true),
                _ => {
                    p.unknown.push("workbuddy:result-status".into());
                    None
                }
            };
            result(
                string(v, "callId"),
                v.get("output")
                    .ok_or(LineErrorKind::MissingField("output"))?,
                error,
                3,
                p,
            )?;
        }
        "reasoning" | "file-history-snapshot" => native_content(
            if v["type"] == "reasoning" {
                Some(Role::Assistant)
            } else {
                None
            },
            required(v, "type")?,
            v,
            1,
            p,
        ),
        "function_call" | "function_call_result" | "ai-title" => p
            .ignored
            .push(format!("workbuddy:{}", required(v, "type")?)),
        other => p.unknown.push(format!("workbuddy:{other}")),
    }
    Ok(())
}
fn qoder(v: &Value, state: &mut State, p: &mut Parsed) -> Result<(), LineErrorKind> {
    let kind = v
        .get("type")
        .or_else(|| v.get("role"))
        .and_then(Value::as_str)
        .ok_or(LineErrorKind::MissingField("type or role"))?;
    if let Some(id) = string(v, "sessionId") {
        state.session_id = Some(id);
    }
    match kind {
        "user" | "assistant" => {
            let mut native = v.clone();
            native["type"] = Value::String(kind.into());
            if native.pointer("/message/role").is_none() {
                native
                    .get_mut("message")
                    .and_then(Value::as_object_mut)
                    .ok_or_else(|| LineErrorKind::InvalidField("message".into()))?
                    .insert("role".into(), Value::String(kind.into()));
            }
            anthropic_session(&native, state, p)
        }
        "runtime-config" => {
            state.model = string(v, "model");
            p.emit(
                0,
                Event::Meta(MetaUpdate {
                    session_id: state.session_id.clone(),
                    model: state.model.clone(),
                    ..Default::default()
                }),
            );
            Ok(())
        }
        "session_meta" => {
            match v.pointer("/data/meta_type").and_then(Value::as_str) {
                Some("session_info" | "slash_command") => {
                    p.ignored.push("qoder:session-metadata".into())
                }
                _ => p.unknown.push("qoder:session-metadata-variant".into()),
            }
            Ok(())
        }
        "workspace-directories" => {
            p.ignored.push("qoder:workspace-directories".into());
            Ok(())
        }
        _ => crate::claude::parse(v, state, p),
    }
}
fn grokbot(v: &Value, p: &mut Parsed) -> Result<(), LineErrorKind> {
    p.record_id = string(v, "id");
    p.message_id = p.record_id.clone();
    p.at = millis(v.get("timestampMs"))?;
    match required(v, "kind")? {
        "message" => {
            let role = match v.get("role").and_then(Value::as_str) {
                Some("user") if v.get("fromAgent").is_none_or(Value::is_null) => Role::User,
                Some("user" | "assistant") => Role::Assistant,
                _ => {
                    p.unknown.push("grokbot:role".into());
                    return Ok(());
                }
            };
            if p.wants(EventKinds::MESSAGE) || p.wants(EventKinds::CONTENT) {
                message(
                    role,
                    v.get("content")
                        .ok_or(LineErrorKind::MissingField("content"))?,
                    v,
                    p,
                )?;
            }
        }
        "send-message" => {
            let m = v
                .get("message")
                .ok_or(LineErrorKind::MissingField("message"))?;
            if m.get("type").and_then(Value::as_str) == Some("text") {
                if p.wants(EventKinds::MESSAGE) || p.wants(EventKinds::CONTENT) {
                    message(
                        Role::Assistant,
                        m.get("content")
                            .ok_or(LineErrorKind::MissingField("content"))?,
                        v,
                        p,
                    )?;
                }
            } else if matches!(
                m.get("type").and_then(Value::as_str),
                Some(
                    "attachment"
                        | "widget"
                        | "connector"
                        | "secret-request"
                        | "auto-review-approval"
                        | "scm-connect"
                        | "local-tool-permission"
                        | "cursor-agent"
                )
            ) {
                native_content(Some(Role::Assistant), required(m, "type")?, m, 1, p);
            } else {
                p.unknown
                    .push(format!("grokbot:send:{}", required(m, "type")?));
            }
        }
        "notice" | "event" | "user-attachment" | "mcp-app" | "feedback" | "voice-call" => {
            native_content(
                if v["kind"] == "user-attachment" || v["kind"] == "voice-call" {
                    Some(Role::User)
                } else {
                    None
                },
                required(v, "kind")?,
                v,
                1,
                p,
            );
        }
        "tool-call" => {
            // The desktop replica stores an outline (name/status/summary), not
            // executable arguments or a complete tool result. Never invent them.
            if v.get("id").and_then(Value::as_str).is_some()
                && v.get("name").and_then(Value::as_str).is_some()
                && matches!(
                    v.get("status").and_then(Value::as_str),
                    Some("pending" | "completed" | "failed")
                )
            {
                call(string(v, "id"), required(v, "name")?, None, 3, p);
                native_content(Some(Role::Assistant), "tool-call", v, 4, p);
            } else {
                p.unknown.push("grokbot:tool-call".into());
            }
        }
        other => p.unknown.push(format!("grokbot:{other}")),
    }
    Ok(())
}

// AI SDK messages stored by Cursor's checkpoint compactor. Tool bodies must be
// removed from prose before projection; the original argument/result stays intact.
pub(crate) fn cursor_cli_prompt(v: &Value, p: &mut Parsed) -> Result<(), LineErrorKind> {
    let Some(blocks) = v.get("content").and_then(Value::as_array) else {
        return api_message(v, p);
    };
    let mut native = v.clone();
    let mut prose = Vec::new();
    for (slot, block) in blocks.iter().enumerate() {
        match block.get("type").and_then(Value::as_str) {
            Some("tool-call") => call(
                string(block, "toolCallId"),
                required(block, "toolName")?,
                block.get("args").or_else(|| block.get("input")),
                slot + 3,
                p,
            ),
            Some("tool-result") => result(
                string(block, "toolCallId"),
                block
                    .get("result")
                    .or_else(|| block.get("output"))
                    .ok_or(LineErrorKind::MissingField("tool result"))?,
                block.get("isError").and_then(Value::as_bool),
                slot + 3,
                p,
            )?,
            _ => prose.push(block.clone()),
        }
    }
    if !prose.is_empty() {
        native["content"] = Value::Array(prose);
        api_message(&native, p)?;
    }
    Ok(())
}
fn zed(v: &Value, p: &mut Parsed) -> Result<(), LineErrorKind> {
    let (role, m) = if let Some(m) = v.get("User") {
        (Role::User, m)
    } else if let Some(m) = v.get("Agent") {
        (Role::Assistant, m)
    } else if v.as_str() == Some("Resume") || v.get("Compaction").is_some() {
        native_content(
            None,
            if v.as_str() == Some("Resume") {
                "Resume"
            } else {
                "Compaction"
            },
            v,
            1,
            p,
        );
        return Ok(());
    } else {
        p.unknown.push("zed:message-variant".into());
        return Ok(());
    };
    p.message_id = string(m, "id");
    let content = m
        .get("content")
        .and_then(Value::as_array)
        .ok_or(LineErrorKind::MissingField("content"))?;
    let mut prose = Vec::new();
    for (slot, part) in content.iter().enumerate() {
        if let Some(text) = part.get("Text") {
            prose.push(text.clone());
        } else if let Some(mention) = part.get("Mention") {
            prose.push(
                mention
                    .get("content")
                    .ok_or(LineErrorKind::MissingField("mention content"))?
                    .clone(),
            );
        } else if let Some(tool) = part.get("ToolUse") {
            if !p.wants(EventKinds::TOOL_CALL) {
                continue;
            }
            let input = tool
                .get("input")
                .ok_or(LineErrorKind::MissingField("tool input"))?;
            // The native SDK accepts raw JSON as well as its two-field
            // {type,value} encoding. Arbitrary object keys are tool arguments.
            let arguments = match input.as_object().filter(|o| o.len() == 2) {
                Some(o)
                    if o.get("type").and_then(Value::as_str) == Some("json")
                        && o.contains_key("value") =>
                {
                    ToolArgs::Json(o["value"].clone())
                }
                Some(o)
                    if o.get("type").and_then(Value::as_str) == Some("text")
                        && o.contains_key("value") =>
                {
                    ToolArgs::RawString(required(input, "value")?.into())
                }
                _ => ToolArgs::Json(input.clone()),
            };
            p.emit(
                slot + 3,
                Event::ToolCall(ToolCall {
                    kind: ToolCallKind::Function,
                    id: string(tool, "id"),
                    name: required(tool, "name")?.into(),
                    arguments,
                }),
            );
        } else if part.get("Image").is_some()
            || part.get("Thinking").is_some()
            || part.get("RedactedThinking").is_some()
        {
            native_content(
                Some(role),
                if part.get("Image").is_some() {
                    "Image"
                } else if part.get("Thinking").is_some() {
                    "Thinking"
                } else {
                    "RedactedThinking"
                },
                part,
                slot + 3,
                p,
            );
        } else {
            p.unknown.push("zed:content-variant".into());
        }
    }
    if !prose.is_empty() {
        message(role, &Value::Array(prose), m, p)?;
    }
    if p.wants(EventKinds::TOOL_RESULT)
        && let Some(results) = m.get("tool_results").and_then(Value::as_object)
    {
        for (slot, (id, tool)) in results.iter().enumerate() {
            let content = tool
                .get("content")
                .ok_or(LineErrorKind::MissingField("tool result content"))?;
            let text = if let Some(items) = content.as_array() {
                items
                    .iter()
                    .filter_map(|v| v.as_str().or_else(|| v.get("Text").and_then(Value::as_str)))
                    .collect::<String>()
            } else {
                content
                    .as_str()
                    .or_else(|| content.get("Text").and_then(Value::as_str))
                    .unwrap_or_default()
                    .into()
            };
            p.emit(
                content.as_array().map_or(3, |v| v.len() + 3) + slot,
                Event::ToolResult(ToolResult {
                    call_id: Some(id.clone()),
                    text,
                    output: Some(tool.clone()),
                    is_error: tool.get("is_error").and_then(Value::as_bool),
                }),
            );
        }
    }
    Ok(())
}

fn antigravity(v: &Value, p: &mut Parsed) -> Result<(), LineErrorKind> {
    if let Some(at) = v.pointer("/metadata/createdAt").and_then(Value::as_str) {
        p.at = Some(
            DateTime::parse_from_rfc3339(at)
                .map_err(|_| LineErrorKind::InvalidField("createdAt".into()))?
                .with_timezone(&Utc),
        );
        p.timestamp_text = Some(at.into());
    }
    if let Some(user) = v.get("userInput") {
        let items = user
            .get("items")
            .and_then(Value::as_array)
            .ok_or(LineErrorKind::MissingField("userInput.items"))?;
        message(Role::User, &Value::Array(items.clone()), user, p)?;
    } else if let Some(agent) = v.get("plannerResponse") {
        p.message_id = string(agent, "messageId");
        if let Some(text) = agent.get("response") {
            message(Role::Assistant, text, agent, p)?;
        }
        if let Some(calls) = agent.get("toolCalls").and_then(Value::as_array) {
            for (slot, tool) in calls.iter().enumerate() {
                call(
                    string(tool, "id"),
                    required(tool, "name")?,
                    tool.get("argumentsJson"),
                    slot + 3,
                    p,
                );
            }
        }
        if agent.get("thinking").is_some() {
            native_content(
                Some(Role::Assistant),
                "thinking",
                &agent["thinking"],
                3 + agent
                    .get("toolCalls")
                    .and_then(Value::as_array)
                    .map_or(0, Vec::len),
                p,
            );
        }
    } else if let Some(tool) = v.pointer("/metadata/toolCall") {
        let id = string(tool, "id");
        call(
            id.clone(),
            required(tool, "name")?,
            tool.get("argumentsJson"),
            3,
            p,
        );
        // Keep the original result payload(s), including failures. Metadata may
        // contain internal request headers and is deliberately excluded.
        let output = Value::Object(
            v.as_object()
                .ok_or_else(|| LineErrorKind::InvalidField("step".into()))?
                .iter()
                .filter(|(k, _)| {
                    !matches!(
                        k.as_str(),
                        "type"
                            | "metadata"
                            | "permissions"
                            | "requestedInteraction"
                            | "completedInteractions"
                    )
                })
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
        );
        if matches!(
            v.get("status").and_then(Value::as_str),
            Some("CORTEX_STEP_STATUS_DONE" | "CORTEX_STEP_STATUS_ERROR")
        ) {
            result(
                id,
                &output,
                v.get("error").map(|_| true).or(Some(false)),
                4,
                p,
            )?;
        } else {
            p.ignored.push("antigravity:pending-tool".into());
        }
    } else if v.get("errorMessage").is_some() {
        if let Some(text) = v
            .pointer("/errorMessage/error/userErrorMessage")
            .or_else(|| v.pointer("/errorMessage/error/shortError"))
        {
            message(Role::Assistant, text, v, p)?;
        }
    } else if v.get("checkpoint").is_some() || v.get("conversationHistory").is_some() {
        native_content(None, required(v, "type")?, v, 1, p);
    } else {
        p.unknown
            .push(format!("antigravity:{}", required(v, "type")?));
    }
    if p.wants(EventKinds::USAGE)
        && let Some(native) = v.pointer("/metadata/modelUsage")
    {
        fn count(v: &Value, key: &str) -> Result<Option<u64>, LineErrorKind> {
            match v.get(key) {
                None => Ok(None),
                Some(v) => v
                    .as_u64()
                    .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
                    .map(Some)
                    .ok_or_else(|| LineErrorKind::InvalidField(key.into())),
            }
        }
        usage(
            TokenCounts {
                input: count(native, "inputTokens")?,
                output: count(native, "outputTokens")?,
                reasoning: count(native, "thinkingOutputTokens")?,
                ..Default::default()
            },
            TokenSemantics::Unknown,
            string(native, "model"),
            string(native, "responseId"),
            p,
        );
    }
    Ok(())
}
