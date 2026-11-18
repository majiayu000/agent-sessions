//! OpenHands CLI conversation directories. Event shapes follow openhands-sdk
//! 1.21.0 (`openhands/sdk/event/**`) and a conversation written by OpenHands CLI
//! 1.16.0; all values are synthetic.
use agent_sessions::*;
use serde_json::{Value, json};
use std::path::Path;

fn write(dir: &Path, base: Value, events: &[(u64, Value)]) {
    std::fs::create_dir_all(dir.join("events")).unwrap();
    std::fs::write(dir.join("base_state.json"), base.to_string()).unwrap();
    for (i, e) in events {
        let name = format!("event-{i:05}-{}.json", e["id"].as_str().unwrap());
        std::fs::write(dir.join("events").join(name), e.to_string()).unwrap();
    }
}

fn base() -> Value {
    json!({"id":"c1","workspace":{"working_dir":"/w"},"execution_status":"finished",
        "stats":{"usage_to_metrics":{"agent":{"model_name":"m","accumulated_token_usage":{"prompt_tokens":15},
            "token_usages":[{"model":"m","prompt_tokens":10,"completion_tokens":2,"cache_read_tokens":4,
                "cache_write_tokens":0,"reasoning_tokens":1,"context_window":0,"per_turn_token":12,"response_id":"r1"}]}}}})
}

fn kinds(result: &SessionImport) -> Vec<&str> {
    result
        .events
        .iter()
        .filter_map(|e| match &e.value {
            Event::Content(c) => Some(c.kind.as_str()),
            _ => None,
        })
        .collect()
}

#[test]
fn openhands_messages_actions_observations_and_call_ledger() {
    let dir = tempfile::tempdir().unwrap();
    let text = |t: &str| json!([{"type":"text","text":t,"cache_prompt":false}]);
    write(
        dir.path(),
        base(),
        &[
            (
                0,
                json!({"kind":"SystemPromptEvent","id":"e0","timestamp":"2026-11-17T13:59:20.1","source":"agent",
                "system_prompt":{"type":"text","text":"sys","cache_prompt":false},"tools":[]}),
            ),
            (
                1,
                json!({"kind":"MessageEvent","id":"e1","timestamp":"2026-11-17T13:59:21.1","source":"user",
                "llm_message":{"role":"user","content":text("问"),"thinking_blocks":[]},"activated_skills":[],"extended_content":[]}),
            ),
            (
                2,
                json!({"kind":"ActionEvent","id":"e2","timestamp":"2026-11-17T13:59:22.1","source":"agent",
                "thought":text("先看看"),"reasoning_content":"r","thinking_blocks":[],"tool_name":"terminal","tool_call_id":"c",
                "tool_call":{"id":"c","name":"terminal","arguments":"{\"command\":\"ls\"}","origin":"completion"},
                "llm_response_id":"r1","action":{"kind":"TerminalAction","command":"ls"}}),
            ),
            (
                3,
                json!({"kind":"ObservationEvent","id":"e3","timestamp":"2026-11-17T13:59:23.1","source":"environment",
                "tool_name":"terminal","tool_call_id":"c","action_id":"e2",
                "observation":{"kind":"TerminalObservation","content":text("a.txt"),"is_error":false}}),
            ),
            (
                4,
                json!({"kind":"AgentErrorEvent","id":"e4","timestamp":"2026-11-17T13:59:24.1","source":"agent",
                "tool_name":"x","tool_call_id":"d","error":"bad tool"}),
            ),
            // Index past five digits must sort numerically, not as text.
            (
                100000,
                json!({"kind":"MessageEvent","id":"e5","timestamp":"2026-11-17T13:59:25.1","source":"agent",
                "llm_message":{"role":"assistant","content":text("完成"),"thinking_blocks":[]},"llm_response_id":"r2",
                "activated_skills":[],"extended_content":[]}),
            ),
        ],
    );
    let result = import_session(Agent::OpenHands, dir.path(), &ReadOptions::default()).unwrap();
    assert!(result.summary.is_supported(), "{:?}", result.summary);
    let texts: Vec<_> = result
        .events
        .iter()
        .filter_map(|e| match &e.value {
            Event::Message(m) => Some((m.role, m.text.as_str())),
            _ => None,
        })
        .collect();
    assert_eq!(
        texts,
        vec![
            (Role::User, "问"),
            (Role::Assistant, "先看看"),
            (Role::Assistant, "完成")
        ]
    );
    assert!(result.events.iter().any(|e| matches!(&e.value,
        Event::ToolCall(c) if c.id.as_deref() == Some("c") && c.name == "terminal"
            && c.arguments == ToolArgs::RawString("{\"command\":\"ls\"}".into()))));
    let results: Vec<_> = result
        .events
        .iter()
        .filter_map(|e| match &e.value {
            Event::ToolResult(r) => Some((r.call_id.as_deref(), r.is_error, r.text.as_str())),
            _ => None,
        })
        .collect();
    assert_eq!(
        results,
        vec![
            (Some("c"), Some(false), "a.txt"),
            (Some("d"), Some(true), "bad tool")
        ]
    );
    assert_eq!(
        kinds(&result),
        vec!["SystemPromptEvent", "reasoning_content"]
    );
    let usage: Vec<_> = result
        .events
        .iter()
        .filter_map(|e| match &e.value {
            Event::Usage(u) => Some(u),
            _ => None,
        })
        .collect();
    assert_eq!(usage.len(), 1, "accumulated totals are not a second ledger");
    assert_eq!(usage[0].counts.input, Some(10));
    assert_eq!(usage[0].dedup_key.as_deref(), Some("openhands:r1"));
    let first = result
        .events
        .iter()
        .find(|e| matches!(e.value, Event::Message(_)))
        .unwrap();
    assert_eq!(first.at, None, "local time without offset is not converted");
    assert_eq!(
        first.timestamp_text.as_deref(),
        Some("2026-11-17T13:59:21.1")
    );
    // The base_state.json path selects the same conversation.
    let again = import_session(
        Agent::OpenHands,
        &dir.path().join("base_state.json"),
        &ReadOptions::default(),
    )
    .unwrap();
    assert_eq!(again.events.len(), result.events.len());
}

#[test]
fn openhands_unknown_event_kind_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        json!({"id":"c2"}),
        &[(
            0,
            json!({"kind":"FutureEvent","id":"e0","timestamp":"t","source":"agent"}),
        )],
    );
    let result = import_session(Agent::OpenHands, dir.path(), &ReadOptions::default()).unwrap();
    assert!(!result.summary.is_supported());
    assert_eq!(result.summary.unknown_types["openhands:FutureEvent"], 1);
}
