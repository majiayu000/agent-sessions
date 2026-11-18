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

fn claude(rows: &[Value]) -> SessionImport {
    lines(Agent::ClaudeCode, rows)
}

// Claude Code 2.1.30x writes request snapshots beside the transcript: the
// request parameters, the system prompt/tool shape and a hash-addressed copy
// of each request message. They repeat transcript context, so they are kept
// as Content and must not become a second set of messages or tool events.
#[test]
fn claude_request_snapshots_are_content_not_messages() {
    let rows = [
        json!({"type":"user","sessionId":"s","uuid":"u","message":{"role":"user","content":"问"}}),
        json!({"type":"api-request","id":"r","sessionId":"s","timestamp":"2026-11-01T00:00:00Z","version":"2.1.326","base":"u","keep":1,"tail":["h"],"requestDigest":"d","shapeHash":"x","querySource":"sdk","cacheControl":[],"params":{"model":"m","max_tokens":1}}),
        json!({"type":"api-request-blob","hash":"h","sessionId":"s","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"c","is_error":false,"content":"out"}]}}),
        json!({"type":"api-request-blob","hash":"k","sessionId":"s","message":{"role":"assistant","content":[{"type":"thinking","thinking":"t","signature":"g"}]}}),
        json!({"type":"api-request-shape","sessionId":"s","shapeHash":"x","timestamp":"2026-11-01T00:00:00Z","version":"2.1.326","shape":{"system":[{"type":"text","text":"sys"}],"tools":[{"name":"Read","description":"d","input_schema":{}}]}}),
    ];
    let result = claude(&rows);
    assert!(result.summary.is_supported(), "{:?}", result.summary);
    assert_eq!(
        content_kinds(&result),
        vec![
            "api-request",
            "api-request-blob",
            "api-request-blob",
            "api-request-shape"
        ]
    );
    let roles: Vec<_> = result
        .events
        .iter()
        .filter_map(|e| match &e.value {
            Event::Content(c) if c.kind == "api-request-blob" => Some(c.role),
            _ => None,
        })
        .collect();
    assert_eq!(roles, vec![Some(Role::User), Some(Role::Assistant)]);
    let counted = result
        .events
        .iter()
        .filter(|e| matches!(e.value, Event::Message(_) | Event::ToolResult(_)))
        .count();
    assert_eq!(counted, 1, "only the transcript user message is a message");
}

#[test]
fn claude_session_bookkeeping_records_are_classified() {
    let rows = [
        json!({"type":"relocated","sessionId":"s","relocatedCwd":"/p"}),
        json!({"type":"fork-context-ref","agentId":"a","contextLength":3,"parentLastUuid":"u","parentSessionId":"p"}),
        json!({"type":"continued-in","sessionId":"s","continuedInSessionId":"n","timestamp":"2026-11-01T00:00:00Z"}),
        json!({"type":"worktree-state","sessionId":"s","worktreeSession":{"originalCwd":"/p","worktreePath":"/w","worktreeBranch":"b"}}),
        json!({"type":"artifact-comment-monitor","sessionId":"s","v":1,"artifacts":{"a":{"state":"open","title":"t","writtenAtMs":1}}}),
        json!({"type":"artifact-autoreact-ledger","sessionId":"s","v":1,"accountUuid":"x","artifacts":{}}),
        json!({"type":"session-owner","sessionId":"s","accountUuid":"x","organizationUuid":"y"}),
        json!({"type":"tools-used","sessionId":"s","tools":["Read"]}),
        json!({"type":"launched"}),
        json!({"type":"failed","agentId":"a","key":"k"}),
        json!({"type":"tag","sessionId":"s","tag":"label"}),
    ];
    let result = claude(&rows);
    assert!(result.summary.is_supported(), "{:?}", result.summary);
    assert_eq!(
        content_kinds(&result),
        vec![
            "relocated",
            "fork-context-ref",
            "continued-in",
            "worktree-state",
            "artifact-comment-monitor",
            "artifact-autoreact-ledger"
        ]
    );
    for kind in ["session-owner", "tools-used", "launched", "failed", "tag"] {
        assert_eq!(result.summary.ignored_types[kind], 1, "{kind}");
    }
}

// Server-side tools (e.g. the advisor tool) return `<name>_tool_result`
// blocks linked to a `server_tool_use` call; a model switch writes `fallback`.
#[test]
fn claude_server_tool_results_and_model_fallback() {
    let rows = [
        json!({"type":"assistant","sessionId":"s","uuid":"a","message":{"role":"assistant","content":[
            {"type":"server_tool_use","id":"c","name":"advisor","input":{}},
            {"type":"advisor_tool_result","tool_use_id":"c","content":{"type":"advisor_tool_result_error","error_code":"unavailable"}},
            {"type":"fallback","from":{"model":"a"},"to":{"model":"b"}},
            {"type":"text","text":"答"}
        ]}}),
    ];
    let result = claude(&rows);
    assert!(result.summary.is_supported(), "{:?}", result.summary);
    let res = result.events.iter().find_map(|e| match &e.value {
        Event::ToolResult(r) => Some(r),
        _ => None,
    });
    let res = res.expect("server tool result");
    assert_eq!(res.call_id.as_deref(), Some("c"));
    assert_eq!(res.is_error, Some(true));
    assert_eq!(content_kinds(&result), vec!["fallback"]);
}

// Paginated Codex history stores an attached local image as a path reference.
#[test]
fn codex_completed_user_message_keeps_local_image_reference() {
    let rows = [
        json!({"type":"session_meta","payload":{"id":"s","history_mode":"paginated"}}),
        json!({"type":"event_msg","payload":{"type":"item_completed","item":{"type":"UserMessage","id":"u","content":[
            {"type":"Text","text":"看图"},{"type":"local_image","path":"/tmp/a.png"}]}}}),
    ];
    let result = lines(Agent::Codex, &rows);
    assert!(result.summary.is_supported(), "{:?}", result.summary);
    assert_eq!(content_kinds(&result), vec!["local_image"]);
}

// Voice realtime sessions write start/close markers with no transcript text.
#[test]
fn codex_realtime_session_markers_are_ignored_but_other_items_stay_unknown() {
    let rows = [
        json!({"type":"realtime_item","ordinal":1,"timestamp":"2026-11-01T00:00:00Z","payload":{"type":"realtime_session_started","id":"r","realtime_session_id":"x"}}),
        json!({"type":"realtime_item","ordinal":2,"timestamp":"2026-11-01T00:00:01Z","payload":{"type":"realtime_session_closed","id":"r","realtime_session_id":"x","outcome":"ok"}}),
    ];
    let result = lines(Agent::Codex, &rows);
    assert!(result.summary.is_supported(), "{:?}", result.summary);
    assert_eq!(result.summary.ignored_types["realtime_item"], 2);
    let other = lines(
        Agent::Codex,
        &[json!({"type":"realtime_item","payload":{"type":"realtime_transcript","text":"x"}})],
    );
    assert!(!other.summary.is_supported());
}
