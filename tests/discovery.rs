use agent_sessions::*;
use std::{fs, process::Command};

#[test]
fn discovers_configured_roots_archives_and_subagents() {
    let tmp = tempfile::tempdir().unwrap();
    let roots = Roots::from_home(tmp.path());
    for rel in [
        ".claude/projects/p/main.jsonl",
        ".claude/projects/p/subagents/s.jsonl",
        ".codex/sessions/2026/a.jsonl",
        ".codex/archived_sessions/b.jsonl",
    ] {
        let path = tmp.path().join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, b"{}\n").unwrap();
    }
    let d = discover(&roots, &DiscoverFilter::default());
    assert!(d.errors.is_empty());
    assert_eq!(d.files.len(), 3);
    let d = discover(
        &roots,
        &DiscoverFilter {
            include_subagents: true,
            ..Default::default()
        },
    );
    assert_eq!(d.files.len(), 4);
    assert_eq!(
        d.files
            .iter()
            .filter(|f| f.kind == FileKind::Subagent)
            .count(),
        1
    );
    let path = tmp.path().join(".claude/projects/p/main.jsonl");
    assert_eq!(
        classify(&path, &roots).unwrap().unwrap().agent,
        Agent::ClaudeCode
    );
    assert!(
        classify(&tmp.path().join("outside.jsonl"), &roots)
            .unwrap()
            .is_none()
    );
    assert!(
        classify(
            &tmp.path().join(".claude/projects/../outside.jsonl"),
            &roots
        )
        .is_err()
    );
}

#[test]
fn missing_roots_are_normal_but_wrong_type_is_visible() {
    let tmp = tempfile::tempdir().unwrap();
    let roots = Roots::from_home(tmp.path());
    assert!(
        discover(&roots, &DiscoverFilter::default())
            .errors
            .is_empty()
    );
    fs::create_dir_all(tmp.path().join(".claude")).unwrap();
    fs::write(tmp.path().join(".claude/projects"), "not a directory").unwrap();
    let result = discover(&roots, &DiscoverFilter::default());
    assert_eq!(result.errors.len(), 1);
    assert_eq!(result.errors[0].operation, DiscoverOperation::ReadDirectory);
}

#[cfg(unix)]
#[test]
fn symlink_cycle_is_not_followed() {
    use std::os::unix::fs::symlink;
    let tmp = tempfile::tempdir().unwrap();
    let roots = Roots::from_home(tmp.path());
    let dir = tmp.path().join(".claude/projects");
    fs::create_dir_all(&dir).unwrap();
    symlink(&dir, dir.join("loop")).unwrap();
    fs::write(dir.join("main.jsonl"), "{}").unwrap();
    symlink(dir.join("main.jsonl"), dir.join("copy.jsonl")).unwrap();
    let d = discover(&roots, &DiscoverFilter::default());
    assert_eq!(d.files.len(), 1);
}

#[test]
fn file_read_uses_fresh_size_not_discovery_snapshot() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let file = SessionFile::inspect(Agent::ClaudeCode, tmp.path()).unwrap();
    fs::write(tmp.path(), b"123456").unwrap();
    let error = read(
        &file,
        &ReadOptions {
            max_file_bytes: Some(2),
            ..Default::default()
        },
    )
    .err()
    .unwrap();
    assert!(matches!(error, ReadError::TooLarge { limit: 2 }));
}

#[test]
fn env_overrides_are_tested_in_isolated_processes() {
    for empty in [false, true] {
        let output = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "env_child", "--ignored"])
            .env(
                "CLAUDE_CONFIG_DIR",
                if empty {
                    ""
                } else {
                    "/tmp/agent-fixture-claude"
                },
            )
            .env("CODEX_HOME", "/tmp/agent-fixture-codex")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
    }
}

#[test]
#[ignore = "invoked in isolated processes by env_overrides_are_tested_in_isolated_processes"]
fn env_child() {
    if std::env::var_os("CLAUDE_CONFIG_DIR").unwrap().is_empty() {
        assert!(Roots::from_env().is_err());
    } else {
        let r = Roots::from_env().unwrap();
        assert_eq!(
            r.claude.unwrap().to_str(),
            Some("/tmp/agent-fixture-claude")
        );
        assert_eq!(r.codex.unwrap().to_str(), Some("/tmp/agent-fixture-codex"));
    }
}
