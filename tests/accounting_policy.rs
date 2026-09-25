use agent_sessions::*;
use serde_json::json;
use std::io::Cursor;

fn usage(agent: Agent, data: String, policy: AccountingPolicy) -> Vec<Usage> {
    read_from(
        agent,
        Cursor::new(data),
        &ReadOptions {
            include: EventKinds::USAGE,
            accounting: policy,
            ..Default::default()
        },
    )
    .unwrap()
    .map(|r| r.unwrap())
    .filter_map(|r| match r.value {
        Event::Usage(u) => Some(u),
        _ => None,
    })
    .collect()
}
#[test]
fn statistics_conversion_is_explicit_and_auditable() {
    let row = json!({"timestamp":"2026-01-01T00:00:00.000Z","message":{"id":"m","usage":{
        "input_tokens":-10,"output_tokens":5,"cache_creation_input_tokens":3,
        "cache_creation":{"ephemeral_1h_input_tokens":9}}}})
    .to_string();
    let rows = usage(
        Agent::ClaudeCode,
        row.clone(),
        AccountingPolicy::UsageStatistics,
    );
    assert_eq!(rows[0].counts.input, Some(0));
    assert_eq!(rows[0].counts.output, Some(5));
    assert_eq!(rows[0].counts.cache_write_1h, Some(3));
    assert_eq!(rows[0].counts.cache_read, None);
    assert_eq!(rows[0].adjustments.len(), 2);
    let mut strict =
        read_from(Agent::ClaudeCode, Cursor::new(row), &ReadOptions::default()).unwrap();
    assert!(strict.next().unwrap().is_err());
}
#[test]
fn statistics_missing_fields_stay_unknown_in_native_usage() {
    let rows = usage(
        Agent::ClaudeCode,
        json!({"message":{"usage":{}}}).to_string(),
        AccountingPolicy::UsageStatistics,
    );
    assert!(rows[0].counts.is_missing());
    assert_eq!(
        rows[0].adjustments,
        vec![UsageAdjustment::UnreportedCounters]
    );
}
#[test]
fn cumulative_statistics_policy_preserves_reset_contract() {
    let row = |n| {
        json!({"type":"event_msg","payload":{"type":"token_count","info":{
        "total_token_usage":{"input_tokens":n}}}})
        .to_string()
    };
    let rows = usage(
        Agent::Codex,
        [row(100), row(20), row(30)].join("\n"),
        AccountingPolicy::UsageStatistics,
    );
    assert_eq!(
        rows.iter().map(|u| u.counts.input).collect::<Vec<_>>(),
        vec![Some(100), Some(0), Some(10)]
    );
    assert!(
        rows[1]
            .adjustments
            .contains(&UsageAdjustment::CumulativeRegression)
    );
}
#[test]
fn native_timestamp_and_record_uuid_do_not_replace_message_identity() {
    let row = json!({"type":"assistant","uuid":"record","timestamp":"2026-01-01T00:00:00.000Z",
        "message":{"content":[],"usage":{"input_tokens":1}}})
    .to_string();
    let mut r = read_from(
        Agent::ClaudeCode,
        Cursor::new(row),
        &ReadOptions {
            include: EventKinds::USAGE,
            ..Default::default()
        },
    )
    .unwrap();
    let event = r.next().unwrap().unwrap();
    assert_eq!(
        event.timestamp_text.as_deref(),
        Some("2026-01-01T00:00:00.000Z")
    );
    assert_eq!(event.record_id.as_deref(), Some("record"));
    assert!(event.message_id.is_none());
}
#[test]
fn codex_model_fallback_skips_empty_values() {
    let rows = usage(
        Agent::Codex,
        json!({"type":"event_msg","payload":{"type":"token_count","model":"fallback",
        "info":{"model":" ","model_name":"model-two","total_token_usage":{"input_tokens":2}}}})
        .to_string(),
        AccountingPolicy::Strict,
    );
    assert_eq!(rows[0].model.as_deref(), Some("model-two"));
}
