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

#[test]
fn archival_projection_keeps_empty_meta_and_tolerant_text() {
    let row = json!({"type":"user","isMeta":true,"timestamp":"bad","createdAt":100,"message":{"content":[{}, {"type":"text","text":"你好"},{"type":"output_text","text":42},{"type":"input_text","text":"next"}]}});
    let message = project_conversation(&row).unwrap();
    assert_eq!(message.role, Role::User);
    assert_eq!(message.text, "你好\nnext");
    assert_eq!(message.created_at_epoch, None);
    assert_eq!(
        project_conversation(&json!({"type":"assistant"}))
            .unwrap()
            .text,
        ""
    );
    assert!(
        project_conversation(
            &json!({"type":"response_item","payload":{"type":"message","role":"developer"}})
        )
        .is_none()
    );
    assert!(
        project_conversation(&json!({"type":"response_item","payload":{"content":[]}})).is_none()
    );
    for value in [
        json!(i64::MAX),
        json!(" 42 "),
        json!(" 2026-01-01T00:00:00Z "),
    ] {
        assert!(tolerant_timestamp_epoch(&json!({"timestamp":value})).is_some());
    }
    assert_eq!(
        tolerant_timestamp_epoch(&json!({"timestamp":null,"createdAt":42})),
        None
    );
}

#[test]
fn native_function_projection_preserves_types_and_missing_fields() {
    for arguments in [json!("{}"), json!({"cmd":"true"}), json!(null)] {
        let row = json!({"type":"response_item","payload":{"type":"function_call","arguments":arguments}});
        assert_eq!(
            project_codex_function(&row),
            Some(CodexFunction::Call {
                call_id: None,
                name: None,
                arguments: row.pointer("/payload/arguments")
            })
        );
    }
    let row = json!({"type":"response_item","payload":{"type":"function_call_output","call_id":"one","output":{"text":"not a string"}}});
    let Some(CodexFunction::Output { call_id, output }) = project_codex_function(&row) else {
        panic!("output projection")
    };
    assert_eq!(call_id, Some("one"));
    assert!(output.unwrap().is_object());
    assert!(
        project_codex_function(
            &json!({"type":"response_item","payload":{"type":"custom_tool_call"}})
        )
        .is_none()
    );
}

#[test]
fn transcript_preserves_legacy_and_role_specific_text_and_metadata() {
    let legacy =
        json!({"type":"response_item","payload":{"content":[{"type":"text","text":"legacy"}]}});
    assert_eq!(
        project_transcript(Agent::Codex, &legacy)
            .message
            .unwrap()
            .text,
        "legacy"
    );
    let current = json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"user"},{"type":"output_text","text":"excluded"}]}});
    assert_eq!(
        project_transcript(Agent::Codex, &current)
            .message
            .unwrap()
            .text,
        "user"
    );
    let meta = json!({"type":"session_meta","model":"old-model","timestamp":"bad","payload":{"timestamp":"2026-01-01T00:00:00.123Z","source":"exec","originator":"Codex Desktop"}});
    let projected = project_transcript(Agent::Codex, &meta);
    assert_eq!(projected.meta.origin, Some(Origin::Exec));
    assert_eq!(projected.meta.model.as_deref(), Some("old-model"));
    assert_eq!(projected.at.unwrap().timestamp_subsec_millis(), 123);
    let claude = json!({"type":"user","isMeta":true,"message":{"content":[{"type":"text","text":""},{"type":"text","text":"你好"},{"type":"input_text","text":"excluded"}]}});
    let message = project_transcript(Agent::ClaudeCode, &claude)
        .message
        .unwrap();
    assert!(message.is_meta);
    assert_eq!(message.first_text(), Some(""));
    assert_eq!(message.text, "\n你好");
}

#[test]
fn explicit_directory_preserves_root_and_reports_absence() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("subagents/explicit");
    std::fs::create_dir_all(root.join("subagents")).unwrap();
    std::fs::write(root.join("one.jsonl"), "{}").unwrap();
    std::fs::write(root.join("subagents/two.jsonl"), "{}").unwrap();
    let result = discover_directory(Agent::ClaudeCode, &root, &DiscoverFilter::default());
    assert_eq!(result.files.len(), 1);
    assert!(result.errors.is_empty());
    assert_eq!(result.files[0].kind, FileKind::Subagent);
    let result = discover_directory(
        Agent::Codex,
        &root.join("missing"),
        &DiscoverFilter::default(),
    );
    assert_eq!(result.errors.len(), 1);
    assert_eq!(
        result.errors[0].operation,
        DiscoverOperation::DirectoryMetadata
    );
}

#[test]
fn history_preserves_native_out_of_range_timestamp_and_physical_lines() {
    let opts = HistoryOptions {
        strict_fields: false,
        ..Default::default()
    };
    let data = format!("\u{2003}\n{{\"ts\":{}}}\n", i64::MAX);
    // Rust strings cannot contain invalid UTF-8; append the failing byte separately.
    let mut data = data.into_bytes();
    data.push(255);
    let mut reader = read_history_from(Agent::Codex, std::io::Cursor::new(data), &opts).unwrap();
    let row = reader.next().unwrap().unwrap();
    assert_eq!(row.location.line_no, 2);
    assert_eq!(row.value.timestamp, Some(i64::MAX));
    assert_eq!(row.value.at, None);
    assert!(matches!(
        reader.next().unwrap(),
        Err(StreamError::Line {
            line_no: 3,
            kind: LineErrorKind::InvalidUtf8,
            ..
        })
    ));
}

#[test]
fn history_io_error_reports_physical_line_after_blank_records() {
    struct FailsAtEnd(std::io::Cursor<Vec<u8>>);
    impl std::io::Read for FailsAtEnd {
        fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
            self.0.read(bytes)
        }
    }
    impl std::io::BufRead for FailsAtEnd {
        fn fill_buf(&mut self) -> std::io::Result<&[u8]> {
            if self.0.position() as usize == self.0.get_ref().len() {
                Err(std::io::Error::other("synthetic read failure"))
            } else {
                self.0.fill_buf()
            }
        }
        fn consume(&mut self, amount: usize) {
            self.0.consume(amount);
        }
    }
    let source = FailsAtEnd(std::io::Cursor::new(b"\n\n".to_vec()));
    let mut reader = read_history_from(Agent::Codex, source, &HistoryOptions::default()).unwrap();
    assert!(matches!(reader.next(), Some(Err(StreamError::Io(_)))));
    assert_eq!(reader.next_line_no(), 3);
}
