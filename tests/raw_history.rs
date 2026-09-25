use agent_sessions::*;
use std::io::Cursor;

#[test]
fn raw_preserves_invalid_json_unknown_utf8_and_crlf() {
    let data = b"{bad}\r\n\xff\n{\"future\":true}";
    let mut r = read_raw_from(Cursor::new(data), &RawReadOptions::default()).unwrap();
    let records = r.by_ref().collect::<Result<Vec<_>, _>>().unwrap();
    assert_eq!(
        records
            .iter()
            .flat_map(|r| r.bytes.clone())
            .collect::<Vec<_>>(),
        data
    );
    assert_eq!(records[1].byte_start, 7);
    assert!(!records[2].terminated);
    assert_eq!(r.finish().status, ReadStatus::Complete);
}
#[test]
fn raw_resume_labels_absolute_offsets_without_reinterpreting_records() {
    let options = RawReadOptions {
        start_offset: 40,
        stop_at_byte: Some(46),
        ..Default::default()
    };
    let mut r = read_raw_from(Cursor::new(b"{}\n[]\nignored"), &options).unwrap();
    let rows = r.by_ref().collect::<Result<Vec<_>, _>>().unwrap();
    assert_eq!((rows[0].byte_start, rows[1].byte_end), (40, 46));
    let s = r.finish();
    assert_eq!(s.bytes_read, 6);
    assert_eq!(s.delivered_through, 46);
}
#[test]
fn raw_file_snapshot_detects_short_source() {
    let options = RawReadOptions {
        stop_at_byte: Some(10),
        ..Default::default()
    };
    let mut r = read_raw_from(Cursor::new(b"{}\n"), &options).unwrap();
    assert!(r.next().unwrap().is_ok());
    assert!(matches!(
        r.next(),
        Some(Err(StreamError::SnapshotTruncated { .. }))
    ));
    assert_eq!(r.finish().status, ReadStatus::Failed);
}
#[test]
fn raw_oversized_rows_do_not_advance_delivered_prefix() {
    let options = RawReadOptions {
        max_line_bytes: Some(4),
        ..Default::default()
    };
    let mut r = read_raw_from(Cursor::new(b"123456\n{}\n"), &options).unwrap();
    assert!(r.next().unwrap().is_err());
    assert!(r.next().unwrap().is_ok());
    assert!(r.next().is_none());
    assert_eq!(r.finish().delivered_through, 0);
}
#[test]
fn history_timestamp_units_and_repeated_ids_are_preserved() {
    let claude = br#"{"sessionId":"same","timestamp":1000,"display":"text","project":"/fixture"}"#;
    let e = read_history_from(
        Agent::ClaudeCode,
        Cursor::new(claude),
        &HistoryOptions::default(),
    )
    .unwrap()
    .next()
    .unwrap()
    .unwrap();
    assert_eq!(e.at.unwrap().timestamp(), 1);
    assert_eq!(e.value.text.as_deref(), Some("text"));
    let codex = b"{\"session_id\":\"same\",\"ts\":1000}\n{\"session_id\":\"same\",\"ts\":1001}\n";
    let rows = read_history_from(Agent::Codex, Cursor::new(codex), &HistoryOptions::default())
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].at.unwrap().timestamp(), 1000);
    assert!(rows[0].value.text.is_none());
    assert_ne!(rows[0].location, rows[1].location);
}
#[test]
fn history_partial_tail_does_not_commit_the_partial_record() {
    let options = HistoryOptions {
        tail: TailMode::AllowIncomplete,
        ..Default::default()
    };
    let mut r = read_history_from(Agent::Codex, Cursor::new(b"{\"ts\":1}\n{"), &options).unwrap();
    assert!(r.next().unwrap().is_ok());
    assert!(r.next().is_none());
    let s = r.finish();
    assert_eq!(s.status, ReadStatus::IncompleteTail);
    assert_eq!(s.last_complete_byte, 9);
}
#[test]
fn history_errors_and_legacy_field_policy_are_explicit() {
    let v = serde_json::json!({"ts":"bad"});
    assert!(history_entry(Agent::Codex, &v, true).is_err());
    let e = history_entry(Agent::Codex, &v, false).unwrap();
    assert_eq!(e.invalid_fields, vec!["ts"]);
    assert!(e.at.is_none());
    let mut r = read_history_from(
        Agent::Codex,
        Cursor::new(b"bad\n{\"ts\":1}\n"),
        &HistoryOptions::default(),
    )
    .unwrap();
    assert!(r.next().unwrap().is_err());
    assert!(r.next().unwrap().is_ok());
    assert!(r.next().is_none());
    let s = r.finish();
    assert_eq!(s.status, ReadStatus::CompleteWithErrors);
    assert_eq!(s.last_complete_byte, 0);
}
