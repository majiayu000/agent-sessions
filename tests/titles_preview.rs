use agent_sessions::*;
use std::{fs, io::Cursor};

#[test]
fn title_indices_preserve_source_and_last_nonempty_append_order() {
    let home = tempfile::tempdir().unwrap();
    let roots = Roots::from_home(home.path());
    let codex = roots.codex.as_ref().unwrap();
    fs::create_dir_all(codex).unwrap();
    fs::write(
        codex.join("session_index.jsonl"),
        concat!(
            "{\"id\":\"one\",\"thread_name\":\"old\"}\n",
            "{\"id\":\"one\",\"thread_name\":\" 新标题 \"}\n",
            "{\"id\":\"one\",\"thread_name\":\"  \"}\n"
        ),
    )
    .unwrap();
    let t = load_session_titles(Agent::Codex, &roots, &["one".into()]).unwrap();
    assert_eq!(t["one"].text, "新标题");
    assert_eq!(t["one"].origin, SessionTitleOrigin::SourceTitle);
    let project = roots.claude.as_ref().unwrap().join("projects/p");
    fs::create_dir_all(&project).unwrap();
    fs::write(
        project.join("sessions-index.json"),
        r#"{"entries":[{"sessionId":"one","summary":" indexed "}]}"#,
    )
    .unwrap();
    fs::write(project.join("one.jsonl"), "unreadable transcript body").unwrap();
    let t =
        load_session_titles(Agent::ClaudeCode, &roots, &["one".into(), "absent".into()]).unwrap();
    assert_eq!(t.len(), 1);
    assert_eq!(t["one"].text, "indexed");
    assert_eq!(t["one"].origin, SessionTitleOrigin::SourceSummary);
}
#[test]
fn malformed_title_error_does_not_print_field_values() {
    let home = tempfile::tempdir().unwrap();
    let roots = Roots::from_home(home.path());
    let codex = roots.codex.as_ref().unwrap();
    fs::create_dir_all(codex).unwrap();
    fs::write(
        codex.join("session_index.jsonl"),
        r#"{"id":42,"thread_name":"PRIVATE-SENTINEL"}"#,
    )
    .unwrap();
    let e = load_session_titles(Agent::Codex, &roots, &["one".into()]).unwrap_err();
    assert!(!e.to_string().contains("PRIVATE-SENTINEL"));
    assert_eq!(e.kind(), std::io::ErrorKind::InvalidData);
    assert!(e.to_string().starts_with("Malformed title index "));
}
#[test]
fn first_text_block_retains_empty_and_unicode_boundaries() {
    for (body, first) in [
        (
            "[{\"type\":\"text\",\"text\":\"你好\"},{\"type\":\"text\",\"text\":\"two\"}]",
            "你好",
        ),
        (
            "[{\"type\":\"text\",\"text\":\"\"},{\"type\":\"text\",\"text\":\"two\"}]",
            "",
        ),
    ] {
        let data = format!("{{\"type\":\"user\",\"message\":{{\"content\":{body}}}}}");
        let mut r = read_from(
            Agent::ClaudeCode,
            Cursor::new(data),
            &ReadOptions::default(),
        )
        .unwrap();
        let Event::Message(m) = r.next().unwrap().unwrap().value else {
            panic!("message")
        };
        assert_eq!(m.first_text(), Some(first));
        assert_eq!(m.text_segments.len(), 2);
    }
}
