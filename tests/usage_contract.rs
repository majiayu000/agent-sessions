use agent_sessions::*;
use serde_json::{Value, json};
use std::io::Cursor;

fn run(agent: Agent, rows: &[Value], mode: CodexUsageMode) -> (Vec<Usage>, ReadSummary) {
    let data = rows
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    let mut r = read_from(
        agent,
        Cursor::new(data),
        &ReadOptions {
            codex_usage: mode,
            ..Default::default()
        },
    )
    .unwrap();
    let usage = r
        .by_ref()
        .filter_map(|r| match r.ok()?.value {
            Event::Usage(u) => Some(u),
            _ => None,
        })
        .collect();
    (usage, r.finish())
}
fn total(n: u64) -> Value {
    json!({"type":"event_msg","payload":{"type":"token_count","info":{
    "total_token_usage":{"input_tokens":n,"cached_input_tokens":0,"output_tokens":0}}}})
}

#[test]
fn codex_prefers_last_and_deduplicates_full_cumulative_vector() {
    let mut second = total(200);
    second["payload"]["info"]["last_token_usage"] = json!({"input_tokens":7,"output_tokens":1});
    let (u, s) = run(
        Agent::Codex,
        &[total(100), second.clone(), second, total(250)],
        CodexUsageMode::TokenCount,
    );
    assert!(s.is_complete());
    assert_eq!(u.len(), 3);
    assert_eq!(u[0].basis, UsageBasis::InitialCumulative);
    assert_eq!(u[1].basis, UsageBasis::LastSample);
    assert_eq!(u[1].counts.input, Some(7));
    assert_eq!(u[2].basis, UsageBasis::CumulativeDelta);
    assert_eq!(u[2].counts.input, Some(50));
}

#[test]
fn absent_counts_remain_distinct_from_zero() {
    let rows =
        [json!({"type":"assistant","message":{"id":"m","content":[],"usage":{"input_tokens":0}}})];
    let (u, s) = run(Agent::ClaudeCode, &rows, CodexUsageMode::TokenCount);
    assert!(s.is_complete());
    assert_eq!(u[0].counts.input, Some(0));
    assert_eq!(u[0].counts.output, None);
    assert_eq!(u[0].counts.cache_write_1h, None);
}

#[test]
fn response_and_legacy_ledgers_are_never_combined() {
    let response = json!({"type":"token_usage_record","payload":{"response_id":"r1","usage":{"input_tokens":9}}});
    let rows = [total(100), response];
    let (legacy, s) = run(Agent::Codex, &rows, CodexUsageMode::TokenCount);
    assert!(s.is_complete());
    assert_eq!(legacy.len(), 1);
    assert_eq!(legacy[0].counts.input, Some(100));
    let (response, s) = run(Agent::Codex, &rows, CodexUsageMode::Response);
    assert!(s.is_complete());
    assert_eq!(response.len(), 1);
    assert_eq!(response[0].counts.input, Some(9));
    assert_eq!(response[0].dedup_key.as_deref(), Some("codex:r1"));
}

#[test]
fn regression_rebaselines_without_fabricating_usage() {
    let (u, s) = run(
        Agent::Codex,
        &[total(100), total(20), total(30)],
        CodexUsageMode::TokenCount,
    );
    assert_eq!(s.line_errors, 1);
    assert_eq!(u.len(), 2);
    assert_eq!(u[1].counts.input, Some(10));
}

#[test]
fn corrupt_gap_rebaselines_once_then_recovers() {
    let bad = json!({"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":-2}}}});
    let (u, s) = run(
        Agent::Codex,
        &[total(100), bad, total(200), total(220)],
        CodexUsageMode::TokenCount,
    );
    assert_eq!(s.line_errors, 2);
    assert_eq!(u.len(), 2);
    assert_eq!(u[1].counts.input, Some(20));
}

#[test]
fn model_change_keeps_session_baseline_and_current_model() {
    let rows = [
        json!({"type":"turn_context","payload":{"model":"a"}}),
        total(100),
        json!({"type":"turn_context","payload":{"model":"b"}}),
        total(125),
    ];
    let (u, s) = run(Agent::Codex, &rows, CodexUsageMode::TokenCount);
    assert!(s.is_complete());
    assert_eq!(u[1].model.as_deref(), Some("b"));
    assert_eq!(u[1].counts.input, Some(25));
}

#[test]
fn claude_repeated_message_usage_is_tagged_not_silently_removed() {
    let row = json!({"type":"assistant","message":{"id":"m1","content":"x","usage":{
        "input_tokens":5,"cache_creation_input_tokens":4,"cache_creation":{"ephemeral_1h_input_tokens":2}}}});
    let (u, s) = run(
        Agent::ClaudeCode,
        &[row.clone(), row],
        CodexUsageMode::TokenCount,
    );
    assert!(s.is_complete());
    assert_eq!(u.len(), 2);
    assert_eq!(u[0].dedup_key, u[1].dedup_key);
    assert_eq!(u[0].counts.cache_write_1h, Some(2));
}

#[test]
fn exclusive_normalization_never_guesses_or_double_counts() {
    let counts = TokenCounts {
        input: Some(100),
        cache_read: Some(30),
        cache_write: Some(10),
        output: Some(50),
        reasoning: Some(20),
        ..Default::default()
    };
    let exclusive = counts.exclusive(TokenSemantics::CodexInclusive).unwrap();
    assert_eq!(exclusive.input, Some(60));
    assert_eq!(exclusive.output, Some(30));
    let partial = TokenCounts {
        cache_write: None,
        ..counts
    }
    .exclusive(TokenSemantics::CodexInclusive)
    .unwrap();
    assert_eq!(partial.input, None);
    assert!(
        TokenCounts {
            input: Some(1),
            ..counts
        }
        .exclusive(TokenSemantics::CodexInclusive)
        .is_none()
    );
}

#[test]
fn invalid_known_usage_reports_error_instead_of_disappearing() {
    let rows = [json!({"type":"assistant","message":{"content":"text","usage":{}}})];
    let (u, s) = run(Agent::ClaudeCode, &rows, CodexUsageMode::TokenCount);
    assert!(u.is_empty());
    assert_eq!(s.line_errors, 1);
    assert!(!s.is_complete());
}
