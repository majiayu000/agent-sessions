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

#[test]
fn collected_title_errors_preserve_valid_results_and_strict_default() {
    let home = tempfile::tempdir().unwrap();
    let roots = Roots::from_home(home.path());
    let codex = roots.codex.as_ref().unwrap();
    let claude = roots.claude.as_ref().unwrap().join("projects/p");
    fs::create_dir_all(codex).unwrap();
    fs::create_dir_all(&claude).unwrap();
    fs::write(
        codex.join("session_index.jsonl"),
        concat!(
            "{\"id\":\"one\",\"thread_name\":\"old\"}\n",
            "{\"id\":\"other\",\"thread_name\":42}\n",
            "{\"id\":\"one\",\"thread_name\":\"new\"}\n",
            "{\"id\":\"one\",\"thread_name\":false}\n"
        ),
    )
    .unwrap();
    fs::write(
        claude.join("sessions-index.json"),
        r#"{"entries":[
        {"sessionId":"one","summary":"old"},
        {"sessionId":"other","summary":42},
        {"sessionId":"one","summary":"new"},
        {"sessionId":"one","summary":false}
    ]}"#,
    )
    .unwrap();
    for agent in [Agent::ClaudeCode, Agent::Codex] {
        assert!(load_session_titles(agent, &roots, &["one".into()]).is_err());
        let result = load_session_titles_with_options(
            agent,
            &roots,
            &["one".into()],
            &TitleReadOptions {
                error_mode: TitleErrorMode::Collect,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(result.titles["one"].text, "new");
        assert_eq!(result.errors.len(), 2);
        for error in result.errors {
            assert_eq!(error.source.kind(), std::io::ErrorKind::InvalidData);
            assert!(!error.source.to_string().contains("summary"));
        }
    }
}

#[test]
fn title_limits_are_enforced_and_long_codex_rows_can_be_skipped_explicitly() {
    let home = tempfile::tempdir().unwrap();
    let roots = Roots::from_home(home.path());
    let codex = roots.codex.as_ref().unwrap();
    let claude = roots.claude.as_ref().unwrap().join("projects/p");
    fs::create_dir_all(codex).unwrap();
    fs::create_dir_all(&claude).unwrap();
    let row = "{\"id\":\"one\",\"thread_name\":\"ok\"}\n";
    fs::write(
        codex.join("session_index.jsonl"),
        format!("{}\n{row}", "x".repeat(100)),
    )
    .unwrap();
    fs::write(
        claude.join("sessions-index.json"),
        r#"{"entries":[{"sessionId":"one","summary":"ok"}]}"#,
    )
    .unwrap();
    let options = TitleReadOptions {
        max_line_bytes: Some(row.len()),
        error_mode: TitleErrorMode::Collect,
        ..Default::default()
    };
    let result =
        load_session_titles_with_options(Agent::Codex, &roots, &["one".into()], &options).unwrap();
    assert_eq!(result.titles["one"].text, "ok");
    assert_eq!(result.errors.len(), 1);
    assert_eq!(result.errors[0].line_no, Some(1));
    for agent in [Agent::ClaudeCode, Agent::Codex] {
        let options = TitleReadOptions {
            max_file_bytes: Some(8),
            ..Default::default()
        };
        let error =
            load_session_titles_with_options(agent, &roots, &["one".into()], &options).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("byte limit"));
        let result = load_session_titles_with_options(
            agent,
            &roots,
            &["one".into()],
            &TitleReadOptions {
                error_mode: TitleErrorMode::Collect,
                ..options
            },
        )
        .unwrap();
        assert!(result.titles.is_empty());
        assert_eq!(result.errors.len(), 1);
    }
}

#[test]
fn title_file_limit_accepts_exact_boundary_and_empty_selection_never_reads() {
    let home = tempfile::tempdir().unwrap();
    let roots = Roots::from_home(home.path());
    let dir = roots.codex.as_ref().unwrap();
    fs::create_dir_all(dir).unwrap();
    let row = "{\"id\":\"one\",\"thread_name\":\"ok\"}\n";
    fs::write(dir.join("session_index.jsonl"), row).unwrap();
    let options = TitleReadOptions {
        max_file_bytes: Some(row.len() as u64),
        max_line_bytes: Some(row.len()),
        ..Default::default()
    };
    let result =
        load_session_titles_with_options(Agent::Codex, &roots, &["one".into()], &options).unwrap();
    assert_eq!(result.titles["one"].text, "ok");
    assert!(result.errors.is_empty());
    let result = load_session_titles_with_options(
        Agent::Codex,
        &roots,
        &[],
        &TitleReadOptions {
            max_file_bytes: Some(0),
            ..options
        },
    )
    .unwrap();
    assert!(result.titles.is_empty());
    assert!(result.errors.is_empty());
}

#[test]
fn collected_title_syntax_errors_are_visible_without_leaking_values() {
    let home = tempfile::tempdir().unwrap();
    let roots = Roots::from_home(home.path());
    let codex = roots.codex.as_ref().unwrap();
    let projects = roots.claude.as_ref().unwrap().join("projects");
    fs::create_dir_all(codex).unwrap();
    fs::create_dir_all(projects.join("bad")).unwrap();
    fs::create_dir_all(projects.join("good")).unwrap();
    fs::write(
        codex.join("session_index.jsonl"),
        concat!(
            "{\"id\":\"other\",\"thread_name\": PRIVATE-SENTINEL}\n",
            "{\"id\":\"one\",\"thread_name\":\"ok\"}\n"
        ),
    )
    .unwrap();
    fs::write(
        projects.join("bad/sessions-index.json"),
        "{\"entries\": PRIVATE-SENTINEL}",
    )
    .unwrap();
    fs::write(
        projects.join("good/sessions-index.json"),
        r#"{"entries":[{"sessionId":"one","summary":"ok"}]}"#,
    )
    .unwrap();
    for agent in [Agent::ClaudeCode, Agent::Codex] {
        let result = load_session_titles_with_options(
            agent,
            &roots,
            &["one".into()],
            &TitleReadOptions {
                error_mode: TitleErrorMode::Collect,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(result.titles["one"].text, "ok");
        assert_eq!(result.errors.len(), 1);
        assert!(!format!("{:?}", result.errors).contains("PRIVATE-SENTINEL"));
    }
}
