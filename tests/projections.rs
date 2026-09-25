use agent_sessions::*;
use serde_json::json;
use std::io::Cursor;

#[test]
fn usage_only_accepts_native_usage_record_without_message_content() {
    let row = json!({"type":"assistant","message":{"id":"fixture","usage":{"input_tokens":1}}})
        .to_string();
    let opts = ReadOptions {
        include: EventKinds::USAGE,
        ..Default::default()
    };
    let mut reader = read_from(Agent::ClaudeCode, Cursor::new(row), &opts).unwrap();
    let events = reader.by_ref().collect::<Result<Vec<_>, _>>().unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].location.event_index, 2);
    assert!(reader.finish().is_complete());
}

#[test]
fn malformed_unselected_payload_does_not_block_messages() {
    let row =
        json!({"type":"assistant","message":{"content":"keep","usage":{"input_tokens":"bad"}}})
            .to_string();
    let opts = ReadOptions {
        include: EventKinds::MESSAGE,
        ..Default::default()
    };
    let events = read_from(Agent::ClaudeCode, Cursor::new(row), &opts)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert!(matches!(&events[0].value,Event::Message(m) if m.text=="keep"));
}

#[test]
fn malformed_meta_flag_is_an_error_not_false() {
    let row = json!({"type":"user","isMeta":"true","message":{"content":"context"}}).to_string();
    let mut reader =
        read_from(Agent::ClaudeCode, Cursor::new(row), &ReadOptions::default()).unwrap();
    assert!(matches!(
        reader.next(),
        Some(Err(StreamError::Line {
            kind: LineErrorKind::InvalidField(_),
            ..
        }))
    ));
}

#[test]
fn usage_does_not_inherit_a_different_claude_messages_model() {
    let rows = [
        json!({"type":"assistant","message":{"model":"model-one","usage":{"input_tokens":1}}}),
        json!({"type":"assistant","message":{"usage":{"input_tokens":2}}}),
    ];
    let data = rows
        .iter()
        .map(|v| v.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    let opts = ReadOptions {
        include: EventKinds::USAGE,
        ..Default::default()
    };
    let events = read_from(Agent::ClaudeCode, Cursor::new(data), &opts)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert!(matches!(&events[1].value,Event::Usage(u) if u.model.is_none()));
}

#[test]
fn all_projection_diagnostics_and_unknown_counters_still_work() {
    let data = "{\"type\":\"future\"}\n{\"type\":\"assistant\",\"message\":{\"usage\":{}}}\n";
    let opts = ReadOptions {
        include: EventKinds::USAGE,
        ..Default::default()
    };
    let mut reader = read_from(Agent::ClaudeCode, Cursor::new(data), &opts).unwrap();
    assert!(reader.next().unwrap().is_err());
    assert!(reader.next().is_none());
    let summary = reader.finish();
    assert_eq!(summary.unknown_types["future"], 1);
    assert_eq!(summary.line_errors, 1);
}
