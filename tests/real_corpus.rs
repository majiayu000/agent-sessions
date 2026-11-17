//! Regressions for record shapes first seen in a read-only sweep of real local
//! histories (`examples/corpus_sweep.rs`). Fixtures are synthetic: they copy
//! the native field structure, never the original text, IDs or payloads.
use agent_sessions::*;
use serde_json::{Value, json};
use std::io::Cursor;

fn lines(agent: Agent, rows: &[Value]) -> SessionImport {
    let text = rows
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    import_session_from(agent, Cursor::new(text), &ReadOptions::default()).unwrap()
}

fn content_kinds(result: &SessionImport) -> Vec<&str> {
    result
        .events
        .iter()
        .filter_map(|e| match &e.value {
            Event::Content(c) => Some(c.kind.as_str()),
            _ => None,
        })
        .collect()
}

// WorkBuddy 5.x writes CodeBuddy SDK session metadata (host kind, runtime,
// title history) as its own record; it carries no conversation text.
#[test]
fn workbuddy_session_meta_is_native_content_not_unknown() {
    let meta = json!({"type":"session-meta","id":"m","sessionId":"s","timestamp":1700000000123_i64,
        "meta":{"codebuddy.ai/hostKind":"unopted","codebuddy.ai/agentRuntime":"local-agent",
            "codebuddy.ai/sessionTitle":[{"type":"ai-title","id":"t","aiTitle":"title","timestamp":1700000000123_i64}]}});
    let result = lines(
        Agent::WorkBuddy,
        &[
            json!({"type":"message","id":"u","sessionId":"s","timestamp":1700000000000_i64,"role":"user","content":[{"type":"input_text","text":"问"}]}),
            meta.clone(),
        ],
    );
    assert!(result.summary.is_supported(), "{:?}", result.summary);
    assert_eq!(content_kinds(&result), vec!["session-meta"]);
    let messages = result
        .events
        .iter()
        .filter(|e| matches!(e.value, Event::Message(_)))
        .count();
    assert_eq!(messages, 1, "metadata must not become a message");
    let data = result.events.iter().find_map(|e| match &e.value {
        Event::Content(c) => Some(&c.data),
        _ => None,
    });
    assert_eq!(data, Some(&meta));
}

// A tool interrupted before it finished is saved with status "incomplete": the
// call has a result record but no success/failure outcome.
#[test]
fn workbuddy_incomplete_tool_result_keeps_unknown_outcome() {
    let rows = [
        json!({"type":"function_call","callId":"c","name":"run","arguments":"{}"}),
        json!({"type":"function_call_result","callId":"c","status":"incomplete","output":{"type":"text","text":"partial"}}),
    ];
    for agent in [Agent::WorkBuddy, Agent::CodeBuddy] {
        let result = lines(agent, &rows);
        assert!(
            result.summary.is_supported(),
            "{agent:?} {:?}",
            result.summary
        );
        let outcome = result.events.iter().find_map(|e| match &e.value {
            Event::ToolResult(r) => Some((r.call_id.as_deref(), r.is_error, r.text.as_str())),
            _ => None,
        });
        assert_eq!(outcome, Some((Some("c"), None, "partial")), "{agent:?}");
    }
}

#[test]
fn workbuddy_unrecognized_result_status_stays_unknown() {
    let result = lines(
        Agent::WorkBuddy,
        &[json!({"type":"function_call_result","callId":"c","status":"later","output":"x"})],
    );
    assert!(!result.summary.is_supported());
    assert_eq!(result.summary.unknown_types["workbuddy:result-status"], 1);
}
