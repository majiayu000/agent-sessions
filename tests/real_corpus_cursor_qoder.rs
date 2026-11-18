//! Cursor Agent CLI, Cursor IDE and Qoder record shapes first seen in a
//! read-only sweep of real local histories (`examples/corpus_sweep.rs`).
//! Fixtures are synthetic: they copy the native field structure, never the
//! original text, IDs or payloads. Protobuf fixtures are hand-built from vendor
//! field numbers, not from serializer output of the implementation under test.
use agent_sessions::*;
use serde_json::{Value, json};
use std::io::Cursor;
use std::path::Path;

fn texts(result: &SessionImport) -> Vec<(Role, &str)> {
    result
        .events
        .iter()
        .filter_map(|e| match &e.value {
            Event::Message(m) => Some((m.role, m.text.as_str())),
            _ => None,
        })
        .collect()
}
fn tools(result: &SessionImport) -> Vec<&str> {
    result
        .events
        .iter()
        .filter_map(|e| match &e.value {
            Event::ToolCall(c) => Some(c.name.as_str()),
            _ => None,
        })
        .collect()
}
fn contents<'a>(result: &'a SessionImport, kind: &str) -> Vec<&'a Value> {
    result
        .events
        .iter()
        .filter_map(|e| match &e.value {
            Event::Content(c) if c.kind == kind => Some(&c.data),
            _ => None,
        })
        .collect()
}

fn pb_varint(mut n: u64) -> Vec<u8> {
    let mut v = Vec::new();
    while n >= 128 {
        v.push((n as u8) | 128);
        n >>= 7;
    }
    v.push(n as u8);
    v
}
fn pb_bytes(tag: u64, data: &[u8]) -> Vec<u8> {
    let mut v = pb_varint(tag << 3 | 2);
    v.extend(pb_varint(data.len() as u64));
    v.extend(data);
    v
}
fn pb_bool(tag: u64) -> Vec<u8> {
    let mut v = pb_varint(tag << 3);
    v.push(1);
    v
}
fn hex(v: &[u8]) -> String {
    v.iter().map(|b| format!("{b:02x}")).collect()
}

/// A Cursor Agent CLI `store.db` with the given root blob id and blobs.
fn cursor_cli_store(dir: &Path, root_id: &str, blobs: &[(u8, Vec<u8>)]) -> std::path::PathBuf {
    let path = dir.join("store.db");
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch("CREATE TABLE blobs(id TEXT PRIMARY KEY,data BLOB);CREATE TABLE meta(key TEXT PRIMARY KEY,value TEXT);").unwrap();
    let meta = json!({"agentId":"s","latestRootBlobId":root_id,"name":"New Agent","mode":"default",
        "isRunEverything":false,"createdAt":1700000000000_i64,"blobEncryptionKey":"synthetic-secret"});
    db.execute(
        "INSERT INTO meta VALUES('0',?1)",
        [hex(meta.to_string().as_bytes())],
    )
    .unwrap();
    for (id, data) in blobs {
        db.execute(
            "INSERT INTO blobs VALUES(?1,?2)",
            rusqlite::params![hex(&[*id]), data],
        )
        .unwrap();
    }
    path
}

// CLI 2026.10.22 checkpoints set ConversationStateStructure field 46
// (`writes_voice_call_utterances`, bool) and may carry voice-call utterances
// (field 43, text-bearing). The flag is metadata; utterances stay native Content.
#[test]
fn cursor_cli_voice_call_state_fields_are_known() {
    let dir = tempfile::tempdir().unwrap();
    let user = pb_bytes(1, "问".as_bytes());
    let turn = pb_bytes(1, &pb_bytes(1, &[2]));
    let utterance = [
        pb_bytes(1, b"v"),
        pb_varint(2 << 3),
        vec![1],
        pb_bytes(3, "语音".as_bytes()),
    ]
    .concat();
    let root = [pb_bytes(8, &[1]), pb_bytes(43, &utterance), pb_bool(46)].concat();
    let path = cursor_cli_store(dir.path(), "00", &[(0, root), (1, turn), (2, user)]);
    let result = import_database(Agent::CursorCli, &path, "s", &ReadOptions::default()).unwrap();
    assert!(result.summary.is_supported(), "{:?}", result.summary);
    assert_eq!(texts(&result), vec![(Role::User, "问")]);
    let voice = contents(&result, "voiceCallUtterance");
    assert_eq!(voice.len(), 1, "{:?}", result.events);
    assert_eq!(voice[0]["text"], "语音");
    assert_eq!(voice[0]["kind"], "KIND_SEND");
    assert_eq!(
        result.summary.ignored_types["cursor-cli:voice-call-flags"],
        1
    );
}

// Tool variants dropped from the 2026.10.22 definitions (e.g. Pi tools,
// ToolCall field 61) still occur in older checkpoints and must stay decoded.
#[test]
fn cursor_cli_tool_variants_removed_upstream_still_decode() {
    let dir = tempfile::tempdir().unwrap();
    let args = pb_bytes(1, b"p");
    let pi_read = pb_bytes(1, &args);
    let tool = pb_bytes(61, &pi_read);
    let step = pb_bytes(2, &tool);
    let turn = pb_bytes(1, &[pb_bytes(1, &[2]), pb_bytes(2, &[3])].concat());
    let root = pb_bytes(8, &[1]);
    let path = cursor_cli_store(
        dir.path(),
        "00",
        &[(0, root), (1, turn), (2, pb_bytes(1, b"q")), (3, step)],
    );
    let result = import_database(Agent::CursorCli, &path, "s", &ReadOptions::default()).unwrap();
    assert!(result.summary.is_supported(), "{:?}", result.summary);
    assert_eq!(tools(&result), vec!["piReadToolCall"]);
}

// A store the CLI created but never checkpointed keeps the default empty
// `latestRootBlobId`; its prompt blobs are unreachable and are not history.
#[test]
fn cursor_cli_store_without_checkpoint_is_empty_not_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let prompt = serde_json::to_vec(&json!({"role":"system","content":"context"})).unwrap();
    let path = cursor_cli_store(dir.path(), "", &[(5, prompt)]);
    let result = import_database(Agent::CursorCli, &path, "s", &ReadOptions::default()).unwrap();
    assert!(texts(&result).is_empty());
    assert!(tools(&result).is_empty());
    assert!(
        result.summary.unknown_types.is_empty(),
        "{:?}",
        result.summary
    );
    assert!(
        !serde_json::to_string(&result)
            .unwrap()
            .contains("synthetic-secret")
    );
}

// Cursor IDE rebuilds bubbles from agent conversation state with
// `createdAt: ""` (no recorded time). That is an absent timestamp.
#[test]
fn cursor_ide_bubble_with_empty_created_at_has_no_timestamp() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.vscdb");
    let c = rusqlite::Connection::open(&path).unwrap();
    c.execute_batch("CREATE TABLE cursorDiskKV(key TEXT PRIMARY KEY,value BLOB)")
        .unwrap();
    for (key, value) in [
        (
            "composerData:c",
            json!({"_v":18,"fullConversationHeadersOnly":[{"bubbleId":"u","type":1},{"bubbleId":"a","type":2}]}),
        ),
        (
            "bubbleId:c:u",
            json!({"_v":3,"bubbleId":"u","type":1,"text":"问","createdAt":"2026-01-01T00:00:00.000Z"}),
        ),
        (
            "bubbleId:c:a",
            json!({"_v":3,"bubbleId":"a","type":2,"text":"答","createdAt":""}),
        ),
    ] {
        c.execute(
            "INSERT INTO cursorDiskKV VALUES(?1,?2)",
            rusqlite::params![key, value.to_string()],
        )
        .unwrap();
    }
    drop(c);
    let result = import_database(Agent::Cursor, &path, "c", &ReadOptions::default()).unwrap();
    assert!(result.summary.is_supported(), "{:?}", result.summary);
    assert_eq!(
        texts(&result),
        vec![(Role::User, "问"), (Role::Assistant, "答")]
    );
    let answer = result
        .events
        .iter()
        .find(|e| matches!(&e.value, Event::Message(m) if m.role == Role::Assistant))
        .unwrap();
    assert_eq!(answer.at, None);
    assert_eq!(answer.timestamp_text, None);
}

fn qoder(rows: &[Value]) -> SessionImport {
    let text = rows
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    import_session_from(Agent::Qoder, Cursor::new(text), &ReadOptions::default()).unwrap()
}

// Qoder SDK writes `active-leaf` after each appended entry: a pointer to the
// newest transcript entry (`explicit: false`), optionally naming a compact
// boundary. It carries no text and needs no branch selection.
#[test]
fn qoder_automatic_active_leaf_is_metadata() {
    let rows = [
        json!({"type":"user","uuid":"u","parentUuid":null,"sessionId":"s","timestamp":"2026-01-01T00:00:00.000Z","message":{"role":"user","content":"问"}}),
        json!({"type":"active-leaf","sessionId":"s","leafUuid":"u","explicit":false,"timestamp":1767225600000_i64}),
        json!({"type":"assistant","uuid":"a","parentUuid":"u","sessionId":"s","timestamp":"2026-01-01T00:00:01.000Z","message":{"id":"m","role":"assistant","model":"test","content":[{"type":"text","text":"答"}]}}),
        json!({"type":"active-leaf","sessionId":"s","leafUuid":"a","explicit":false,"compactBoundaryUuid":"b","timestamp":1767225601000_i64}),
    ];
    let result = qoder(&rows);
    assert!(result.summary.is_supported(), "{:?}", result.summary);
    assert_eq!(
        texts(&result),
        vec![(Role::User, "问"), (Role::Assistant, "答")]
    );
    assert_eq!(result.summary.ignored_types["qoder:active-leaf"], 2);
}

// A user-selected leaf (`explicit: true`, e.g. after a rewind) means some
// appended entries may be off the visible branch. Not observed in real
// history yet, so it stays unknown instead of silently replaying all entries.
#[test]
fn qoder_explicit_active_leaf_stays_unknown() {
    let rows = [
        json!({"type":"user","uuid":"u","parentUuid":null,"sessionId":"s","timestamp":"2026-01-01T00:00:00.000Z","message":{"role":"user","content":"问"}}),
        json!({"type":"active-leaf","sessionId":"s","leafUuid":"u","explicit":true,"rewound":true,"timestamp":1767225600000_i64}),
    ];
    let result = qoder(&rows);
    assert!(!result.summary.is_supported());
    assert_eq!(
        result.summary.unknown_types["qoder:active-leaf-explicit"],
        1
    );
}
