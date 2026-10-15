//! Read-only, one-file differential check for Claude / Codex response-item history.
//! No model calls, discovery, transcript output, IDs, tool names or paths are printed.
//! This checks selected field projections and byte provenance, not a new-task E2E.
use agent_sessions::{
    Agent, CodexContentMode, Event, ImportSource, ReadOptions, ToolArgs, import_session_from,
};
use serde_json::{Value, json};
use std::{collections::BTreeSet, error::Error, io::Cursor};

fn body(v: &Value) -> String {
    if let Some(s) = v.as_str() {
        return s.to_owned();
    }
    v.as_array()
        .into_iter()
        .flatten()
        .filter(|b| {
            matches!(
                b["type"].as_str(),
                Some("text" | "input_text" | "output_text")
            )
        })
        .filter_map(|b| b["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n")
}
fn args(v: Option<&Value>, raw: bool) -> Value {
    match v {
        Some(Value::String(s)) if raw => json!({"RawString":s}),
        Some(v) => json!({"Json":v}),
        None => json!("Missing"),
    }
}
fn push(out: &mut [Vec<Value>; 4], kind: usize, loc: &Value, data: Value) {
    out[kind].push(json!([loc, data]));
}
fn claude_tools(c: &Value, loc: &Value, out: &mut [Vec<Value>; 4]) {
    for b in c.as_array().into_iter().flatten() {
        match b["type"].as_str() {
            Some("tool_use" | "server_tool_use") => push(
                out,
                1,
                loc,
                json!([b["id"], b["name"], args(b.get("input"), false)]),
            ),
            Some("tool_result") => push(
                out,
                2,
                loc,
                json!([b["tool_use_id"], b["content"], body(&b["content"])]),
            ),
            _ => {}
        }
    }
}
fn main() -> Result<(), Box<dyn Error>> {
    let mut argv = std::env::args_os().skip(1);
    let agent = match argv.next().as_deref().and_then(|s| s.to_str()) {
        Some("claude") => Agent::ClaudeCode,
        Some("codex") => Agent::Codex,
        _ => return Err("usage: verify_native_history <claude|codex> <one-jsonl-file>".into()),
    };
    let path = argv.next().ok_or("one session file required")?;
    if argv.next().is_some() {
        return Err("one file only".into());
    }
    // Capture once, compare the same immutable bytes; no private copy is written.
    let bytes = std::fs::read(path).map_err(|_| "cannot read session")?;
    let mut expected: [Vec<Value>; 4] = Default::default();
    let mut versions = BTreeSet::new();
    let mut start = 0;
    for (index, line) in bytes.split_inclusive(|b| *b == b'\n').enumerate() {
        let loc = json!([index, index + 1, start, start + line.len()]);
        start += line.len();
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        let v: Value = serde_json::from_slice(line).map_err(|_| "invalid source JSON")?;
        if let Some(version) = v
            .get("version")
            .or_else(|| v.pointer("/payload/cli_version"))
            .and_then(Value::as_str)
        {
            // Report only dotted version tokens, never arbitrary source strings.
            if version.chars().all(|c| c.is_ascii_digit() || c == '.') {
                versions.insert(version.to_owned());
            }
        }
        if agent == Agent::ClaudeCode {
            let kind = v["type"].as_str().unwrap_or("");
            if matches!(kind, "user" | "assistant") {
                let c = &v["message"]["content"];
                push(&mut expected, 0, &loc, json!([kind, body(c)]));
                claude_tools(c, &loc, &mut expected);
                for b in c.as_array().into_iter().flatten() {
                    if matches!(
                        b["type"].as_str(),
                        Some(
                            "thinking"
                                | "redacted_thinking"
                                | "image"
                                | "document"
                                | "tool_reference"
                        )
                    ) {
                        push(&mut expected, 3, &loc, b.clone());
                    }
                }
            } else if kind == "progress" {
                claude_tools(
                    &v["data"]["message"]["message"]["content"],
                    &loc,
                    &mut expected,
                );
            }
        } else if v["type"] == "response_item" {
            let p = &v["payload"];
            match p["type"].as_str() {
                Some("message") if matches!(p["role"].as_str(), Some("user" | "assistant")) => {
                    push(
                        &mut expected,
                        0,
                        &loc,
                        json!([p["role"], body(&p["content"])]),
                    )
                }
                Some("function_call" | "custom_tool_call") => {
                    let key = if p["type"] == "function_call" {
                        "arguments"
                    } else {
                        "input"
                    };
                    push(
                        &mut expected,
                        1,
                        &loc,
                        json!([p["call_id"], p["name"], args(p.get(key), true)]),
                    );
                }
                Some("function_call_output" | "custom_tool_call_output") => push(
                    &mut expected,
                    2,
                    &loc,
                    json!([p["call_id"], p["output"], body(&p["output"])]),
                ),
                Some("reasoning") => push(&mut expected, 3, &loc, p.clone()),
                _ => {}
            }
        }
    }
    let opts = ReadOptions {
        codex_content: CodexContentMode::ResponseItems,
        ..Default::default()
    };
    let result = import_session_from(agent, Cursor::new(&bytes), &opts)
        .map_err(|_| "native import failed (private values withheld)")?;
    let mut actual: [Vec<Value>; 4] = Default::default();
    let mut usages = 0;
    for e in &result.events {
        let [ImportSource::Record(l)] = e.sources.as_slice() else {
            return Err("unexpected provenance".into());
        };
        let loc = json!([l.record_index, l.line_no, l.byte_start, l.byte_end]);
        match &e.value {
            Event::Message(m)
                if matches!(
                    m.role,
                    agent_sessions::Role::User | agent_sessions::Role::Assistant
                ) =>
            {
                let role = if m.role == agent_sessions::Role::User {
                    "user"
                } else {
                    "assistant"
                };
                push(&mut actual, 0, &loc, json!([role, m.text]));
            }
            Event::ToolCall(t) => {
                let a = match &t.arguments {
                    ToolArgs::Json(v) => json!({"Json":v}),
                    ToolArgs::RawString(v) => json!({"RawString":v}),
                    ToolArgs::Missing => json!("Missing"),
                    _ => return Err("new argument representation".into()),
                };
                push(&mut actual, 1, &loc, json!([t.id, t.name, a]));
            }
            Event::ToolResult(t) => {
                push(&mut actual, 2, &loc, json!([t.call_id, t.output, t.text]))
            }
            Event::Content(c)
                if matches!(
                    c.kind.as_str(),
                    "thinking"
                        | "redacted_thinking"
                        | "image"
                        | "document"
                        | "tool_reference"
                        | "reasoning"
                ) =>
            {
                push(&mut actual, 3, &loc, c.data.clone())
            }
            Event::Usage(_) => usages += 1,
            _ => {}
        }
    }
    let matched: Vec<_> = (0..4).map(|i| expected[i] == actual[i]).collect();
    let counts: Vec<_> = actual.iter().map(Vec::len).collect();
    println!(
        "{}",
        json!({"agent":agent,"versions":versions,"bytes":bytes.len(),"records":result.summary.lines,
        "projection_order":["user_assistant_text","tool_call_id_name_arguments","tool_result_id_output_text","selected_native_content"],
        "matched_including_physical_offsets":matched,"counts":counts,"usage_events_observed_not_differentially_checked":usages,
        "supported":result.summary.is_supported(),"unknown_type_count":result.summary.unknown_types.len(),
        "evidence":"existing native history, response-item mode; no new task or model call"})
    );
    if !result.summary.is_supported() || matched.contains(&false) || counts[0] == 0 {
        return Err("native history verification failed (private values withheld)".into());
    }
    Ok(())
}
