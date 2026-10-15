//! Read-only, one-file differential check for Claude / Codex response-item history.
//! No model calls, discovery, transcript output, IDs, tool names or paths are printed.
//! This checks selected field projections and byte provenance, not a new-task E2E.
use agent_sessions::{
    Agent, CodexContentMode, Event, ImportSource, ReadOptions, SessionImport, ToolArgs,
    import_session_from,
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
        .filter_map(|b| {
            if let Some(text) = b.as_str() {
                Some(text)
            } else if matches!(
                b["type"].as_str(),
                Some("text" | "input_text" | "output_text" | "Text" | "inputText")
            ) {
                b["text"].as_str()
            } else {
                None
            }
        })
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
fn selected_content(c: &Value, loc: &Value, out: &mut [Vec<Value>; 4]) {
    for block in c.as_array().into_iter().flatten() {
        if matches!(
            block["type"].as_str(),
            Some(
                "thinking"
                    | "redacted_thinking"
                    | "reasoning"
                    | "image"
                    | "document"
                    | "tool_reference"
            )
        ) {
            push(out, 3, loc, block.clone());
        }
    }
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
    verify(agent, &bytes)
}

fn verify(agent: Agent, bytes: &[u8]) -> Result<(), Box<dyn Error>> {
    let opts = ReadOptions {
        codex_content: CodexContentMode::ResponseItems,
        ..Default::default()
    };
    let result = import_session_from(agent, Cursor::new(bytes), &opts)
        .map_err(|_| "native import failed (private values withheld)")?;
    verify_import(agent, bytes, &result)
}

fn verify_import(agent: Agent, bytes: &[u8], result: &SessionImport) -> Result<(), Box<dyn Error>> {
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
            if matches!(kind, "user" | "assistant" | "system") {
                let c = if kind == "system" {
                    v.pointer("/message/content").unwrap_or(&v["content"])
                } else {
                    &v["message"]["content"]
                };
                if kind != "system" {
                    push(&mut expected, 0, &loc, json!([kind, body(c)]));
                    claude_tools(c, &loc, &mut expected);
                }
                selected_content(c, &loc, &mut expected);
            } else if kind == "progress" {
                claude_tools(
                    &v["data"]["message"]["message"]["content"],
                    &loc,
                    &mut expected,
                );
            }
        } else if v["type"] == "user_message" {
            push(&mut expected, 0, &loc, json!(["user", body(&v["content"])]));
            selected_content(&v["content"], &loc, &mut expected);
        } else if v["type"] == "response_item" {
            let p = &v["payload"];
            let legacy_message = p.get("type").is_none() && p.get("content").is_some();
            let kind = if legacy_message {
                Some("message")
            } else {
                p["type"].as_str()
            };
            match kind {
                Some("message") => {
                    let role = if legacy_message {
                        Some("assistant")
                    } else {
                        p["role"].as_str()
                    };
                    if matches!(role, Some("user" | "assistant")) {
                        push(&mut expected, 0, &loc, json!([role, body(&p["content"])]));
                    }
                    selected_content(&p["content"], &loc, &mut expected);
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
                Some("web_search_call" | "tool_search_call") => {
                    let web = p["type"] == "web_search_call";
                    let id = if web {
                        p["id"].as_str()
                    } else {
                        p["call_id"].as_str().or_else(|| p["id"].as_str())
                    };
                    let name = if web { "web_search" } else { "tool_search" };
                    let arguments = p
                        .get(if web { "action" } else { "arguments" })
                        .filter(|value| !value.is_null());
                    push(
                        &mut expected,
                        1,
                        &loc,
                        json!([id, name, args(arguments, true)]),
                    );
                    if web && matches!(p["status"].as_str(), Some("completed" | "failed")) {
                        push(
                            &mut expected,
                            2,
                            &loc,
                            json!([id, p, search_result_text(p)]),
                        );
                    }
                }
                Some("tool_search_output") => {
                    let id = p["call_id"].as_str().or_else(|| p["id"].as_str());
                    push(
                        &mut expected,
                        2,
                        &loc,
                        json!([id, p, search_result_text(p)]),
                    );
                }
                Some("reasoning") => push(&mut expected, 3, &loc, p.clone()),
                _ => {}
            }
        }
    }
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
    if !result.summary.is_supported() || matched.contains(&false) || counts.iter().all(|n| *n == 0)
    {
        return Err("native history verification failed (private values withheld)".into());
    }
    Ok(())
}

fn search_result_text(value: &Value) -> String {
    let content = value.get("text").or_else(|| value.get("content"));
    match content {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(blocks)) => blocks
            .iter()
            .filter_map(|block| {
                if let Some(text) = block.as_str() {
                    return Some(text);
                }
                if block["thought"] == true {
                    return None;
                }
                match block["type"].as_str() {
                    None | Some("text" | "input_text" | "output_text" | "Text" | "inputText") => {
                        block["text"].as_str()
                    }
                    _ => None,
                }
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn history(records: &[Value]) -> String {
        // Exercise physical line/byte offsets, UTF-8 and an unterminated final line.
        format!(
            "\r\n{}",
            records
                .iter()
                .map(Value::to_string)
                .collect::<Vec<_>>()
                .join("\r\n\n")
        )
    }

    #[test]
    fn supported_claude_system_selected_content_matches() {
        let content = json!([
            {"type":"text","text":"系统 fixture"},
            {"type":"thinking","thinking":"fixture"},
            {"type":"redacted_thinking","data":"fixture"},
            {"type":"reasoning","text":"fixture"},
            {"type":"image","url":"fixture"},
            {"type":"document","data":"fixture"},
            {"type":"tool_reference","name":"fixture"}
        ]);
        for system in [
            json!({"type":"system","content":content}),
            json!({"type":"system","message":{"content":content},"content":[]}),
        ] {
            let bytes = history(&[
                json!({"type":"user","message":{"content":"fixture"}}),
                system,
            ]);
            verify(Agent::ClaudeCode, bytes.as_bytes()).unwrap();
        }
    }

    #[test]
    fn supported_claude_user_assistant_reasoning_matches() {
        for role in ["user", "assistant"] {
            let bytes = history(&[json!({"type":role,"message":{"content":[
                {"type":"text","text":"推理 fixture"},
                {"type":"reasoning","text":"fixture"}
            ]}})]);
            verify(Agent::ClaudeCode, bytes.as_bytes()).unwrap();
        }
    }

    #[test]
    fn supported_codex_early_user_message_matches() {
        for content in [
            json!("早期 request"),
            json!([{"type":"input_text","text":"早期 request"}]),
            json!([
                {"type":"input_text","text":"早期 request"},
                {"type":"reasoning","text":"fixture"},
                {"type":"image","url":"fixture"}
            ]),
        ] {
            let bytes = history(&[
                json!({"type":"user_message","content":content}),
                json!({"type":"response_item","payload":{"content":[{"type":"text","text":"early answer"}]}}),
                json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":"current answer"}}),
            ]);
            verify(Agent::Codex, bytes.as_bytes()).unwrap();
        }
    }

    #[test]
    fn supported_histories_without_text_messages_match() {
        let fixtures = [
            (
                Agent::Codex,
                vec![
                    json!({"type":"response_item","payload":{"type":"function_call","call_id":"fixture","name":"fixture","arguments":"{}"}}),
                    json!({"type":"response_item","payload":{"type":"function_call_output","call_id":"fixture","output":"done"}}),
                ],
            ),
            (
                Agent::Codex,
                vec![json!({"type":"response_item","payload":{"type":"reasoning","summary":[]}})],
            ),
            (
                Agent::ClaudeCode,
                vec![
                    json!({"type":"progress","data":{"message":{"message":{"content":[
                        {"type":"tool_use","id":"fixture","name":"fixture","input":{}},
                        {"type":"tool_result","tool_use_id":"fixture","content":"done"}
                    ]}}}}),
                ],
            ),
            (
                Agent::ClaudeCode,
                vec![json!({"type":"system","content":[{"type":"thinking","thinking":"fixture"}]})],
            ),
        ];
        for (agent, records) in fixtures {
            verify(agent, history(&records).as_bytes()).unwrap();
        }
    }

    #[test]
    fn vacuous_histories_are_rejected() {
        for agent in [Agent::ClaudeCode, Agent::Codex] {
            assert!(verify(agent, b" \r\n\n").is_err());
        }
        let bytes = history(&[json!({"type":"system","content":"unselected system text"})]);
        assert!(verify(Agent::ClaudeCode, bytes.as_bytes()).is_err());
        let bytes = history(&[
            json!({"type":"response_item","payload":{"type":"message","role":"system","content":"unselected system text"}}),
        ]);
        assert!(verify(Agent::Codex, bytes.as_bytes()).is_err());
    }

    #[test]
    fn mismatched_text_and_offsets_are_rejected_without_private_values() {
        let bytes =
            history(&[json!({"type":"assistant","message":{"content":"synthetic private text"}})]);
        let import = || {
            import_session_from(
                Agent::ClaudeCode,
                Cursor::new(bytes.as_bytes()),
                &ReadOptions::default(),
            )
            .unwrap()
        };
        let original = import();
        verify_import(Agent::ClaudeCode, bytes.as_bytes(), &original).unwrap();
        for change_offset in [false, true] {
            let mut result = import();
            let event = result
                .events
                .iter_mut()
                .find(|event| matches!(event.value, Event::Message(_)))
                .unwrap();
            if change_offset {
                let ImportSource::Record(loc) = &mut event.sources[0] else {
                    panic!("record source required")
                };
                loc.byte_start += 1;
            } else {
                let Event::Message(message) = &mut event.value else {
                    unreachable!()
                };
                message.text.push_str(" corrupted");
            }
            let error = verify_import(Agent::ClaudeCode, bytes.as_bytes(), &result).unwrap_err();
            assert_eq!(
                error.to_string(),
                "native history verification failed (private values withheld)"
            );
        }
    }

    fn verify_codex_items(items: &[Value]) -> Result<(), Box<dyn Error>> {
        let bytes = items
            .iter()
            .map(|item| format!("{}\r\n", json!({"type":"response_item","payload":item})))
            .collect::<String>();
        verify(Agent::Codex, bytes.as_bytes())
    }

    #[test]
    fn supported_codex_untyped_message_matches() {
        verify_codex_items(&[json!({"content":[{"type":"output_text","text":"early rollout"}]})])
            .unwrap();
    }

    #[test]
    fn supported_codex_search_untyped_text_matches() {
        verify_codex_items(&[
            json!({"type":"message","role":"user","content":"search"}),
            json!({"type":"web_search_call","id":"web","status":"completed","text":[{"text":"found"},{"text":"private thought","thought":true}]}),
            json!({"type":"tool_search_output","call_id":"tool","content":[{"text":"found"}]}),
        ]).unwrap();
    }

    #[test]
    fn supported_codex_nested_selected_content_matches() {
        for role in ["user", "assistant", "system", "developer"] {
            verify_codex_items(&[
                json!({"type":"message","role":"user","content":"fixture"}),
                json!({"type":"message","role":role,"content":[
                    {"type":"output_text","text":"visible"},
                    {"type":"reasoning","text":"reasoning fixture"},
                    {"type":"image","url":"fixture"},
                    {"type":"document","data":"fixture"},
                    {"type":"tool_reference","name":"fixture"},
                    {"type":"thinking","thinking":"fixture"},
                    {"type":"redacted_thinking","data":"fixture"}
                ]}),
            ])
            .unwrap();
        }
    }

    #[test]
    fn supported_codex_search_history_matches_with_physical_offsets() {
        let mut items = vec![
            json!({"type":"message","role":"user","content":[{"type":"input_text","text":"搜索 fixture"}]}),
        ];
        for status in ["in_progress", "completed", "failed"] {
            items.push(json!({"type":"web_search_call","id":status,"status":status,"action":{"type":"search","query":"fixture"}}));
        }
        items.push(json!({"type":"tool_search_call","id":"missing-arguments"}));
        // Native tool search supports both identity fields and missing/raw/JSON arguments.
        for id_field in ["call_id", "id"] {
            for arguments in [Value::Null, json!("raw query"), json!({"query":"fixture"})] {
                items.push(
                    json!({"type":"tool_search_call",id_field:"search","arguments":arguments}),
                );
                let text_field = if id_field == "call_id" {
                    "text"
                } else {
                    "content"
                };
                items.push(json!({"type":"tool_search_output",id_field:"search","status":"completed","tools":[{"name":"fixture"}],text_field:["found", {"type":"output_text","text":"fixture"}]}));
            }
        }
        let bytes = items
            .iter()
            .map(|item| format!("{}\r\n", json!({"type":"response_item","payload":item})))
            .collect::<String>();
        verify(Agent::Codex, bytes.as_bytes()).unwrap();
    }
}
