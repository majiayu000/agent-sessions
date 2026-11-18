//! Read-only aggregate sweep of one agent's native sessions.
//!
//! Prints counts and record-type diagnostics only: never paths, IDs, titles,
//! message text or tool payloads. Usage:
//!
//! ```sh
//! cargo run --locked --release --example corpus_sweep -- AGENT ROOT [MAX_SESSIONS]
//! ```
//!
//! ROOT is a session directory (files are discovered with `discover_directory`)
//! or a native SQLite file. For Cursor Agent CLI, a directory is searched for
//! `store.db` files. Live clients may be appending, so a truncated final JSONL
//! line is tolerated and reported separately.
use agent_sessions::*;
use serde_json::json;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

fn agent(name: &str) -> Option<Agent> {
    Some(match name {
        "claude" => Agent::ClaudeCode,
        "codex" => Agent::Codex,
        "gemini" => Agent::GeminiCli,
        "qwen" => Agent::QwenCode,
        "kimi" => Agent::KimiCli,
        "pi" => Agent::Pi,
        "copilot" => Agent::CopilotCli,
        "codebuddy" => Agent::CodeBuddy,
        "iflow" => Agent::IFlow,
        "opencode" => Agent::OpenCode,
        "cline" => Agent::Cline,
        "roo" => Agent::RooCode,
        "goose" => Agent::Goose,
        "continue" => Agent::Continue,
        "cursor" => Agent::Cursor,
        "grok" => Agent::Grok,
        "cline-cli" => Agent::ClineCli,
        "hermes" => Agent::Hermes,
        "workbuddy" => Agent::WorkBuddy,
        "qoder" => Agent::Qoder,
        "zcode" => Agent::ZCode,
        "grok-bot" => Agent::GrokBot,
        "cursor-cli" => Agent::CursorCli,
        "zed" => Agent::Zed,
        "warp" => Agent::Warp,
        "crush" => Agent::Crush,
        "openhands" => Agent::OpenHands,
        _ => return None,
    })
}

#[derive(Default)]
struct Tally {
    sessions: u64,
    supported: u64,
    unsupported: u64,
    errors: u64,
    empty: u64,
    truncated_tail: u64,
    events: BTreeMap<&'static str, u64>,
    unknown: BTreeMap<String, u64>,
    ignored: BTreeMap<String, u64>,
    error_kinds: BTreeMap<String, u64>,
}

impl Tally {
    fn add(&mut self, result: Result<SessionImport, ImportError>) {
        self.sessions += 1;
        let result = match result {
            Ok(result) => result,
            Err(err) => {
                self.errors += 1;
                *self.error_kinds.entry(error_kind(&err)).or_default() += 1;
                // Local debugging only: the message can contain the source path.
                if std::env::var_os("CORPUS_SWEEP_DEBUG").is_some() {
                    eprintln!("error in session {}: {err}", self.sessions);
                }
                return;
            }
        };
        let summary = &result.summary;
        if summary.is_supported() {
            self.supported += 1;
        } else {
            self.unsupported += 1;
        }
        if summary.truncated_tail {
            self.truncated_tail += 1;
        }
        if result.events.is_empty() {
            self.empty += 1;
        }
        for (kind, n) in &summary.unknown_types {
            *self.unknown.entry(kind.to_string()).or_default() += *n;
        }
        for (kind, n) in &summary.ignored_types {
            *self.ignored.entry(kind.to_string()).or_default() += *n;
        }
        for event in &result.events {
            let name = match &event.value {
                Event::Message(_) => "messages",
                Event::ToolCall(_) => "tool_calls",
                Event::ToolResult(_) => "tool_results",
                Event::Usage(_) => "usage",
                Event::Content(_) => "content",
                _ => "other",
            };
            *self.events.entry(name).or_default() += 1;
        }
    }
}

/// Error category without the message, which can contain paths or payload text.
fn error_kind(err: &ImportError) -> String {
    let debug = format!("{err:?}");
    debug
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .filter(|s| !s.is_empty())
        .take(2)
        .collect::<Vec<_>>()
        .join("::")
}

fn is_database(path: &Path) -> bool {
    path.is_file()
        && matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("db" | "vscdb" | "sqlite")
        )
}

fn store_dbs(root: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            store_dbs(&entry.path(), out);
        } else if kind.is_file() && entry.file_name() == "store.db" {
            out.push(entry.path());
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let usage = "usage: corpus_sweep AGENT ROOT [MAX_SESSIONS]";
    let agent = agent(&args.next().ok_or(usage)?).ok_or("unknown agent")?;
    let root = PathBuf::from(args.next().ok_or(usage)?);
    let max = args.next().map(|s| s.parse::<usize>()).transpose()?;
    let max = max.unwrap_or(usize::MAX);
    let opts = ReadOptions {
        max_file_bytes: None,
        max_line_bytes: Some(64 * 1024 * 1024),
        tail: TailMode::AllowIncomplete,
        ..ReadOptions::default()
    };
    let mut tally = Tally::default();
    let mut discover_errors = 0;
    let databases: Vec<PathBuf> = if is_database(&root) {
        vec![root.clone()]
    } else if agent == Agent::CursorCli {
        let mut found = Vec::new();
        store_dbs(&root, &mut found);
        found.sort();
        found
    } else {
        Vec::new()
    };
    if databases.is_empty() {
        let filter = DiscoverFilter {
            agents: vec![agent],
            include_subagents: true,
            modified_after: None,
        };
        let discovery = discover_directory(agent, &root, &filter);
        discover_errors = discovery.errors.len();
        for file in discovery.files.iter().take(max) {
            tally.add(import_session(agent, &file.path, &opts));
        }
    } else {
        'outer: for db in &databases {
            let sessions = match list_database_sessions(agent, db, &opts) {
                Ok(sessions) => sessions,
                Err(err) => {
                    tally.sessions += 1;
                    tally.errors += 1;
                    *tally.error_kinds.entry(error_kind(&err)).or_default() += 1;
                    continue;
                }
            };
            for session in sessions {
                if tally.sessions as usize >= max {
                    break 'outer;
                }
                tally.add(import_database(agent, db, &session.id, &opts));
            }
        }
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "agent": agent,
            "databases": databases.len(),
            "discover_errors": discover_errors,
            "sessions": tally.sessions,
            "supported": tally.supported,
            "unsupported": tally.unsupported,
            "errors": tally.errors,
            "empty": tally.empty,
            "truncated_tail": tally.truncated_tail,
            "events": tally.events,
            "unknown_types": tally.unknown,
            "ignored_types": tally.ignored,
            "error_kinds": tally.error_kinds,
        }))?
    );
    Ok(())
}
