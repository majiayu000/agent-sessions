use agent_sessions::*;
use std::{fs, io::Cursor, path::Path};

#[test]
fn versioned_synthetic_fixtures_match_complete_events_and_summary() {
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut count = 0;
    for (host, agent) in [("claude", Agent::ClaudeCode), ("codex", Agent::Codex)] {
        for file in fs::read_dir(base.join(host)).unwrap() {
            let path = file.unwrap().path();
            if path.extension().is_none_or(|e| e != "jsonl") {
                continue;
            }
            let mode = if path
                .file_stem()
                .unwrap()
                .to_string_lossy()
                .contains("response")
            {
                CodexUsageMode::Response
            } else {
                CodexUsageMode::TokenCount
            };
            let mut reader = read_from(
                agent,
                Cursor::new(fs::read(&path).unwrap()),
                &ReadOptions {
                    codex_usage: mode,
                    tail: TailMode::AllowIncomplete,
                    ..Default::default()
                },
            )
            .unwrap();
            let events = reader
                .by_ref()
                .map(|r| match r {
                    Ok(e) => serde_json::json!({"event":e}),
                    Err(StreamError::Line {
                        line_no,
                        byte_start,
                        kind,
                    }) => serde_json::json!({
                    "error":kind,"line_no":line_no,"byte_start":byte_start}),
                    Err(e) => panic!("unexpected fatal error in {}: {e}", path.display()),
                })
                .collect::<Vec<_>>();
            let actual = serde_json::json!({"events":events,"summary":reader.finish()});
            let expected_path = path.with_extension("expected.json");
            if std::env::var_os("UPDATE_GOLDEN").is_some() {
                fs::write(
                    &expected_path,
                    format!("{}\n", serde_json::to_string_pretty(&actual).unwrap()),
                )
                .unwrap();
            }
            let expected: serde_json::Value =
                serde_json::from_slice(&fs::read(expected_path).unwrap()).unwrap();
            assert_eq!(actual, expected, "fixture {}", path.display());
            count += 1;
        }
    }
    assert!(count >= 20, "fixture corpus is incomplete");
}
