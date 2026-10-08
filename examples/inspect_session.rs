//! Print only aggregate diagnostics for a native session or one database session.
use agent_sessions::{
    Agent, CodexContentMode, Event, ReadOptions, import_database, import_session,
};
use serde_json::json;
use std::{error::Error, path::PathBuf};

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args_os().skip(1);
    let agent = args
        .next()
        .ok_or("usage: inspect_session <agent> <path> [database-session-id] [completed-items]")?;
    let agent = match agent.to_str() {
        Some("claude") => Agent::ClaudeCode,
        Some("codex") => Agent::Codex,
        Some("gemini") => Agent::GeminiCli,
        Some("qwen") => Agent::QwenCode,
        Some("kimi") => Agent::KimiCli,
        Some("pi") => Agent::Pi,
        Some("copilot") => Agent::CopilotCli,
        Some("codebuddy") => Agent::CodeBuddy,
        Some("iflow") => Agent::IFlow,
        Some("opencode") => Agent::OpenCode,
        Some("cline") => Agent::Cline,
        Some("roo") => Agent::RooCode,
        Some("goose") => Agent::Goose,
        Some("continue") => Agent::Continue,
        Some("cursor") => Agent::Cursor,
        Some("grok") => Agent::Grok,
        Some("cline-cli") => Agent::ClineCli,
        Some("hermes") => Agent::Hermes,
        Some("workbuddy") => Agent::WorkBuddy,
        Some("qoder") => Agent::Qoder,
        Some("zcode") => Agent::ZCode,
        Some("grok-bot") => Agent::GrokBot,
        Some("cursor-cli") => Agent::CursorCli,
        Some("zed") => Agent::Zed,
        Some("warp") => Agent::Warp,
        Some("antigravity") => Agent::Antigravity,
        _ => return Err("unknown agent; see docs/support.md".into()),
    };
    let path = PathBuf::from(args.next().ok_or("session path required")?);
    let id = args.next();
    let mode = args.next();
    if args.next().is_some() {
        return Err("too many arguments".into());
    }
    let mut opts = ReadOptions::default();
    if mode.as_deref() == Some(std::ffi::OsStr::new("completed-items"))
        || id.as_deref() == Some(std::ffi::OsStr::new("completed-items"))
    {
        opts.codex_content = CodexContentMode::CompletedItems;
    } else if mode.is_some() {
        return Err("unknown content mode".into());
    }
    let result = if let Some(id) = id.filter(|id| id != "completed-items") {
        import_database(
            agent,
            &path,
            id.to_str().ok_or("session ID must be UTF-8")?,
            &opts,
        )?
    } else {
        import_session(agent, &path, &opts)?
    };
    let (mut messages, mut calls, mut results, mut usages) = (0_u64, 0_u64, 0_u64, 0_u64);
    for event in &result.events {
        match &event.value {
            Event::Message(_) => messages += 1,
            Event::ToolCall(_) => calls += 1,
            Event::ToolResult(_) => results += 1,
            Event::Usage(_) => usages += 1,
            _ => {}
        }
    }
    println!(
        "{}",
        serde_json::to_string_pretty(
            &json!({"agent":agent,"messages":messages,"tool_calls":calls,"tool_results":results,"usage_events":usages,"supported":result.summary.is_supported(),"summary":result.summary})
        )?
    );
    if !result.summary.is_supported() {
        return Err(
            "source is incomplete or contains unsupported records; inspect diagnostics".into(),
        );
    }
    Ok(())
}
