//! Verify a completed native Codex session against independently known test data.
//! The reply/output files must be expectations, not derived from parsed events.
use agent_sessions::{Agent, Event, ReadOptions, Role, import_session};
use std::{error::Error, path::PathBuf};

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args_os().skip(1);
    let session = PathBuf::from(args.next().ok_or("native session path required")?);
    let reply = PathBuf::from(args.next().ok_or("expected reply file required")?);
    let output = PathBuf::from(args.next().ok_or("expected tool output file required")?);
    if args.next().is_some() {
        return Err("too many arguments".into());
    }
    let reply = std::fs::read_to_string(reply)?;
    let output = std::fs::read_to_string(output)?;
    if reply.is_empty() || output.is_empty() {
        return Err("test expectations must be nonempty".into());
    }
    let result = import_session(Agent::Codex, &session, &ReadOptions::default())?;
    let reply_matches = result.events.iter().any(|e| {
        matches!(&e.value, Event::Message(m) if m.role == Role::Assistant && m.text.trim() == reply.trim())
    });
    let output_matches = result.events.iter().any(|e| {
        let Event::ToolResult(r) = &e.value else {
            return false;
        };
        r.text.contains(&output)
            && r.call_id.as_ref().is_some_and(|id| {
                result
                    .events
                    .iter()
                    .any(|e| matches!(&e.value, Event::ToolCall(c) if c.id.as_ref() == Some(id)))
            })
    });
    let has_usage = result.events.iter().any(|e| {
        matches!(&e.value, Event::Usage(u) if u.counts.input.is_some() && u.counts.output.is_some())
    });
    let supported = result.summary.is_supported();
    println!(
        "reply_matches={reply_matches} matched_tool_output={output_matches} has_usage={has_usage} supported={supported}"
    );
    if !(reply_matches && output_matches && has_usage && supported) {
        return Err("native session failed the expected-content check".into());
    }
    Ok(())
}
