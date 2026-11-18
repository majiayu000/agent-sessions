//! Crush `.crush/crush.db`. The schema is the subset of tables written by Crush
//! 0.101.0 (goose migrations); rows are synthetic.
use agent_sessions::*;
use rusqlite::{Connection, params};
use serde_json::{Value, json};

const SCHEMA: &str = "
CREATE TABLE sessions (id TEXT PRIMARY KEY, parent_session_id TEXT, title TEXT NOT NULL,
  message_count INTEGER NOT NULL DEFAULT 0, prompt_tokens INTEGER NOT NULL DEFAULT 0,
  completion_tokens INTEGER NOT NULL DEFAULT 0, cost REAL NOT NULL DEFAULT 0.0,
  updated_at INTEGER NOT NULL, created_at INTEGER NOT NULL,
  summary_message_id TEXT, todos TEXT, channel TEXT);
CREATE TABLE messages (id TEXT PRIMARY KEY, session_id TEXT NOT NULL, role TEXT NOT NULL,
  parts TEXT NOT NULL default '[]', model TEXT, created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL, finished_at INTEGER, provider TEXT,
  is_summary_message INTEGER DEFAULT 0 NOT NULL, model_name TEXT);";

/// (id, role, parts, model, created_at, is_summary_message)
type Row<'a> = (&'a str, &'a str, Value, Option<&'a str>, i64, i64);

fn db(rows: &[Row]) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("crush.db");
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch(SCHEMA).unwrap();
    conn.execute(
        "INSERT INTO sessions(id,parent_session_id,title,prompt_tokens,completion_tokens,updated_at,created_at) VALUES('s',NULL,'t',90,10,1,1)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO sessions(id,parent_session_id,title,updated_at,created_at) VALUES('child','s','sub',2,2)",
        [],
    )
    .unwrap();
    for (id, role, parts, model, created, summary) in rows {
        conn.execute(
            "INSERT INTO messages(id,session_id,role,parts,model,created_at,updated_at,is_summary_message) VALUES(?1,'s',?2,?3,?4,?5,?5,?6)",
            params![id, role, parts.to_string(), model, created, summary],
        )
        .unwrap();
    }
    (dir, path)
}

#[test]
fn crush_session_text_tools_and_native_parts() {
    let (_dir, path) = db(&[
        // Same second: insertion (rowid) order decides, not the random ID.
        (
            "z",
            "user",
            json!([{"type":"text","data":{"text":"问"}},{"type":"finish","data":{"reason":"stop","time":0}}]),
            None,
            100,
            0,
        ),
        (
            "a",
            "assistant",
            json!([
                {"type":"reasoning","data":{"thinking":"t","signature":"g"}},
                {"type":"text","data":{"text":"答"}},
                {"type":"tool_call","data":{"id":"c","name":"view","input":"{\"file_path\":\"x\"}","provider_executed":false,"finished":true}},
                {"type":"finish","data":{"reason":"tool_use","time":101}}
            ]),
            Some("m"),
            100,
            0,
        ),
        (
            "b",
            "tool",
            json!([{"type":"tool_result","data":{"tool_call_id":"c","name":"view","content":"内容","is_error":false}}]),
            None,
            101,
            0,
        ),
        (
            "y",
            "assistant",
            json!([{"type":"text","data":{"text":"摘要"}}]),
            Some("m"),
            102,
            1,
        ),
    ]);
    let sessions = list_database_sessions(Agent::Crush, &path, &ReadOptions::default()).unwrap();
    assert_eq!(
        sessions.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
        vec!["s", "child"]
    );
    let result = import_database(Agent::Crush, &path, "s", &ReadOptions::default()).unwrap();
    assert!(result.summary.is_supported(), "{:?}", result.summary);
    assert_eq!(
        result.summary.ignored_types["crush:session-token-snapshot"],
        1
    );
    let texts: Vec<_> = result
        .events
        .iter()
        .filter_map(|e| match &e.value {
            Event::Message(m) => Some((m.role, m.text.as_str())),
            _ => None,
        })
        .collect();
    assert_eq!(texts, vec![(Role::User, "问"), (Role::Assistant, "答")]);
    assert!(result.events.iter().any(|e| matches!(&e.value,
        Event::ToolCall(c) if c.id.as_deref() == Some("c") && c.name == "view"
            && c.arguments == ToolArgs::RawString("{\"file_path\":\"x\"}".into()))));
    assert!(result.events.iter().any(|e| matches!(&e.value,
        Event::ToolResult(r) if r.call_id.as_deref() == Some("c") && r.text == "内容" && r.is_error == Some(false))));
    let kinds: Vec<_> = result
        .events
        .iter()
        .filter_map(|e| match &e.value {
            Event::Content(c) => Some(c.kind.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(kinds, vec!["finish", "reasoning", "finish", "summary"]);
    assert!(
        !result
            .events
            .iter()
            .any(|e| matches!(e.value, Event::Usage(_)))
    );
    let first = result
        .events
        .iter()
        .find(|e| matches!(e.value, Event::Message(_)))
        .unwrap();
    assert_eq!(first.at.unwrap().timestamp(), 100);
}

#[test]
fn crush_child_session_is_sidechain_and_unknown_parts_are_reported() {
    let (_dir, path) = db(&[]);
    let conn = Connection::open(&path).unwrap();
    conn.execute(
        "INSERT INTO messages(id,session_id,role,parts,created_at,updated_at) VALUES('m','child','user',?1,3,3)",
        [json!([{"type":"text","data":{"text":"子"}},{"type":"future_part","data":{}}]).to_string()],
    )
    .unwrap();
    drop(conn);
    let result = import_database(Agent::Crush, &path, "child", &ReadOptions::default()).unwrap();
    assert!(!result.summary.is_supported());
    assert_eq!(result.summary.unknown_types["crush:part:future_part"], 1);
    assert!(
        result
            .events
            .iter()
            .any(|e| matches!(&e.value, Event::Message(m) if m.is_sidechain))
    );
    assert!(import_session(Agent::Crush, &path, &ReadOptions::default()).is_err());
}
