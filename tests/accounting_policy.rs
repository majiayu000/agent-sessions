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
        json!({"type":"event_msg","timestamp":"2026-01-01T00:00:00Z","payload":{"type":"token_count","info":{
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

#[test]
fn malformed_model_is_not_silently_replaced_with_unknown_pricing() {
    let row = json!({"message":{"model":42,"usage":{"input_tokens":1}}}).to_string();
    let mut r = read_from(
        Agent::ClaudeCode,
        Cursor::new(row),
        &ReadOptions {
            include: EventKinds::USAGE,
            accounting: AccountingPolicy::UsageStatistics,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(matches!(
        r.next(),
        Some(Err(StreamError::Line {
            kind: LineErrorKind::InvalidField(_),
            ..
        }))
    ));
}

#[test]
fn statistics_checks_missing_time_before_ignoring_incomplete_usage() {
    let row = json!({"type":"event_msg","payload":{"type":"token_count","info":{
        "last_token_usage":{"input_tokens":1}}}})
    .to_string();
    let mut reader = read_from(
        Agent::Codex,
        Cursor::new(row),
        &ReadOptions {
            include: EventKinds::USAGE,
            accounting: AccountingPolicy::UsageStatistics,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(matches!(
        reader.next(),
        Some(Err(StreamError::Line {
            kind: LineErrorKind::MissingField("timestamp"),
            ..
        }))
    ));
}

fn statistics_outcome(
    value: &serde_json::Value,
    include: EventKinds,
) -> (Vec<Located<Event>>, Vec<String>, serde_json::Value) {
    let mut reader = read_from(
        Agent::Codex,
        std::io::Cursor::new(value.to_string()),
        &ReadOptions {
            accounting: AccountingPolicy::UsageStatistics,
            include,
            ..Default::default()
        },
    )
    .unwrap();
    let mut events = Vec::new();
    let mut errors = Vec::new();
    for row in reader.by_ref() {
        match row {
            Ok(event) => events.push(event),
            Err(error) => errors.push(format!("{error:?}")),
        }
    }
    (
        events,
        errors,
        serde_json::to_value(reader.finish()).unwrap(),
    )
}

#[test]
fn statistical_fast_and_general_paths_share_timestamp_diagnostics() {
    for timestamp in [
        json!("100"),
        json!(" -100 "),
        json!("2026-01-01T00:00:00.123Z"),
        json!("broken"),
        json!(100),
        json!(null),
        json!({}),
    ] {
        for payload in [
            json!({"type":"token_count","info":{"total_token_usage":{"input_tokens":10}}}),
            json!({"type":"token_count","info":null}),
            json!({"type":"agent_message"}),
        ] {
            let row = json!({"type":"event_msg","timestamp":timestamp,"payload":payload});
            let fast = statistics_outcome(&row, EventKinds::USAGE);
            let general = statistics_outcome(&row, EventKinds::USAGE.union(EventKinds::MESSAGE));
            assert_eq!(fast, general, "{row}");
        }
    }
    let row = json!({"type":"event_msg","timestamp":"100","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":10}}}});
    let (events, errors, summary) = statistics_outcome(&row, EventKinds::USAGE);
    assert!(errors.is_empty());
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].at.unwrap().timestamp(), 100);
    assert!(matches!(&events[0].value, Event::Usage(usage) if usage.counts.input == Some(10)));
    assert_eq!(summary["status"], "Complete");
    let row = json!({"type":"event_msg","timestamp":"broken","payload":{"type":"agent_message"}});
    let (events, errors, summary) = statistics_outcome(&row, EventKinds::USAGE);
    assert!(events.is_empty());
    assert_eq!(errors.len(), 1);
    assert!(errors[0].contains("InvalidField(\"timestamp\")"));
    assert_eq!(summary["status"], "CompleteWithErrors");
    assert_eq!(summary["last_complete_byte"], 0);
}

#[test]
fn statistical_timestamp_field_precedence_matches_general_decoder() {
    for fields in [
        json!({"created_at":"100"}),
        json!({"createdAt":100}),
        json!({"timestamp":null,"created_at":"broken"}),
        json!({"created_at":null,"createdAt":"broken"}),
        json!({"data":{"message":{"timestamp":"broken"}}}),
        json!({"data":{"message":{"timestamp":100}}}),
    ] {
        for kind in ["agent_message", "token_count"] {
            let mut row = fields.clone();
            row["type"] = json!("event_msg");
            row["payload"] = json!({"type":kind,"timestamp":"2026-01-01T00:00:00Z"});
            for nested in [true, false] {
                if !nested {
                    row["payload"].as_object_mut().unwrap().remove("timestamp");
                }
                assert_eq!(
                    statistics_outcome(&row, EventKinds::USAGE),
                    statistics_outcome(&row, EventKinds::USAGE.union(EventKinds::MESSAGE)),
                    "{row}"
                );
            }
        }
    }
    // Even a valid fallback timestamp cannot replace the native string required
    // by the ccstats token-count contract when info is present.
    let row =
        json!({"type":"event_msg","created_at":100,"payload":{"type":"token_count","info":{}}});
    let (events, errors, _) = statistics_outcome(&row, EventKinds::USAGE);
    assert!(events.is_empty());
    assert!(errors[0].contains("MissingField(\"timestamp\")"));
}
