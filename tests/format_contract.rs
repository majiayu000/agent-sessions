use agent_sessions::*;
use serde_json::json;
use std::io::Cursor;

fn events(agent: Agent, s: &str, include: EventKinds) -> Vec<Located<Event>> {
    read_from(
        agent,
        Cursor::new(s),
        &ReadOptions {
            include,
            ..Default::default()
        },
    )
    .unwrap()
    .collect::<Result<Vec<_>, _>>()
    .unwrap()
}

#[test]
fn codex_origin_preserves_raw_evidence_and_structured_source_wins() {
    let cases = [
        (
            json!({"source":{"subagent":{}},"originator":"Codex Desktop"}),
            Origin::Subagent,
        ),
        (
            json!({"source":"exec","originator":"Codex Desktop"}),
            Origin::Exec,
        ),
        (
            json!({"source":"vscode","originator":"codex-tui"}),
            Origin::Ide,
        ),
        (
            json!({"source":"cli","thread_source":"subagent"}),
            Origin::Subagent,
        ),
        (json!({"originator":"Codex Desktop"}), Origin::Ide),
        (json!({"source":"future"}), Origin::Unknown),
        (json!({"source":"cli"}), Origin::Interactive),
    ];
    for (payload, expected) in cases {
        let row = json!({"type":"session_meta","payload":payload}).to_string();
        let e = events(Agent::Codex, &row, EventKinds::ALL);
        let Event::Meta(m) = &e[0].value else {
            panic!("meta expected")
        };
        assert_eq!(m.origin, Some(expected));
        assert_eq!(m.source, payload.get("source").cloned());
    }
}

#[test]
fn meta_is_kept_and_timestamp_compatibility_is_explicit() {
    for flag in ["isMeta", "is_meta"] {
        let mut row = json!({"type":"user","created_at":"100","message":{"content":"  meta  "}});
        row[flag] = json!(true);
        let e = events(Agent::ClaudeCode, &row.to_string(), EventKinds::ALL);
        assert_eq!(e[0].at.unwrap().timestamp(), 100);
        assert!(matches!(&e[0].value,Event::Message(m) if m.is_meta && m.text=="  meta  "));
    }
}

#[test]
fn canonical_event_index_survives_filtering() {
    let row=json!({"type":"assistant","cwd":"/work","message":{"content":[
        {"type":"text","text":"hello"},{"type":"tool_use","id":"t","name":"Read","input":{"path":"a"}}
    ]}}).to_string();
    let all = events(Agent::ClaudeCode, &row, EventKinds::ALL);
    let calls = events(Agent::ClaudeCode, &row, EventKinds::TOOL_CALL);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0], all[2]);
}

#[test]
fn codex_tool_arguments_are_not_repaired_or_reinterpreted() {
    let row = json!({"type":"response_item","payload":{"type":"function_call","name":"exec",
        "call_id":"t1","arguments":"{incomplete"}})
    .to_string();
    let e = events(Agent::Codex, &row, EventKinds::ALL);
    assert!(
        matches!(&e[0].value,Event::ToolCall(t) if t.arguments==ToolArgs::RawString("{incomplete".into()))
    );
}

#[test]
fn legacy_progress_and_tool_results_are_separate_events() {
    let row = json!({"type":"progress","data":{"message":{"message":{"id":"m","content":[
        {"type":"tool_use","id":"t","name":"Read","input":{}}
    ]}}}})
    .to_string();
    assert!(matches!(
        &events(Agent::ClaudeCode, &row, EventKinds::ALL)[0].value,
        Event::ToolCall(_)
    ));
    let row = json!({"type":"user","message":{"content":[{"type":"tool_result",
        "tool_use_id":"t","content":"result","is_error":true}]}})
    .to_string();
    let e = events(Agent::ClaudeCode, &row, EventKinds::ALL);
    assert!(matches!(&e[0].value,Event::Message(m) if m.text.is_empty()));
    assert!(
        matches!(&e[1].value,Event::ToolResult(t) if t.text=="result" && t.is_error==Some(true))
    );
}

#[test]
fn unknown_diagnostics_are_bounded_and_content_drift_is_visible() {
    let mut rows = (0..500)
        .map(|i| json!({"type":format!("unknown-{i}")}).to_string())
        .collect::<Vec<_>>();
    rows.push(json!({"type":"user","message":{"content":[{"type":"future_content"}]}}).to_string());
    let mut r = read_from(
        Agent::ClaudeCode,
        Cursor::new(rows.join("\n")),
        &ReadOptions::default(),
    )
    .unwrap();
    for event in r.by_ref() {
        event.unwrap();
    }
    let s = r.finish();
    assert_eq!(s.unknown_types.len(), 129);
    assert_eq!(s.unknown_types.values().sum::<u64>(), 501);
}
