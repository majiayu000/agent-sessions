//! Read-only aggregate scanner. Never prints paths, messages, IDs or arguments.
use agent_sessions::*;
use std::{collections::BTreeMap, time::Instant};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let agents = match args.first().map(String::as_str).unwrap_or("all") {
        "claude" => vec![Agent::ClaudeCode],
        "codex" => vec![Agent::Codex],
        "all" => vec![],
        _ => {
            return Err(
                "usage: scan [all|claude|codex] [legacy|response] [max-files] [line-MiB]".into(),
            );
        }
    };
    let codex_usage = match args.get(1).map(String::as_str).unwrap_or("legacy") {
        "legacy" => CodexUsageMode::TokenCount,
        "response" => CodexUsageMode::Response,
        _ => return Err("usage mode must be legacy or response".into()),
    };
    let max_files = args
        .get(2)
        .map(|s| s.parse::<usize>())
        .transpose()?
        .unwrap_or(usize::MAX);
    let max_line_bytes = args
        .get(3)
        .map(|s| s.parse::<usize>())
        .transpose()?
        .unwrap_or(8)
        .checked_mul(1024 * 1024)
        .ok_or("line budget overflow")?;
    let start = Instant::now();
    let discovery = discover(
        &Roots::from_env()?,
        &DiscoverFilter {
            agents,
            include_subagents: true,
            modified_after: None,
        },
    );
    let mut kinds = BTreeMap::<String, u64>::new();
    let mut unknown = BTreeMap::<String, u64>::new();
    let mut statuses = BTreeMap::<String, u64>::new();
    let mut errors = BTreeMap::<String, u64>::new();
    let mut bytes = 0;
    let mut files = 0;
    let mut open_errors = 0;
    for file in discovery.files.iter().take(max_files) {
        files += 1;
        let Ok(mut reader) = read(
            file,
            &ReadOptions {
                tail: TailMode::AllowIncomplete,
                codex_usage,
                max_line_bytes: Some(max_line_bytes),
                ..Default::default()
            },
        ) else {
            open_errors += 1;
            continue;
        };
        for event in reader.by_ref() {
            match event {
                Ok(e) => {
                    let kind = match e.value {
                        Event::Meta(_) => "meta",
                        Event::Message(_) => "message",
                        Event::Usage(_) => "usage",
                        Event::ToolCall(_) => "tool_call",
                        Event::ToolResult(_) => "tool_result",
                        _ => "other",
                    };
                    *kinds.entry(kind.into()).or_default() += 1;
                }
                Err(StreamError::Line { kind, .. }) => {
                    *errors.entry(format!("{kind:?}")).or_default() += 1
                }
                Err(_) => *errors.entry("fatal_stream_error".into()).or_default() += 1,
            }
        }
        let summary = reader.finish();
        bytes += summary.bytes_read;
        *statuses.entry(format!("{:?}", summary.status)).or_default() += 1;
        for (ty, n) in summary.unknown_types {
            *unknown.entry(ty).or_default() += n;
        }
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "files":files,"bytes":bytes,"events":kinds,"status":statuses,
            "line_errors":errors,"open_errors":open_errors,"discovery_errors":discovery.errors.len(),
            "unknown_types":unknown,"elapsed_ms":start.elapsed().as_millis()
        }))?
    );
    if !errors.is_empty() || open_errors > 0 || !discovery.errors.is_empty() {
        return Err("scan completed with errors; see aggregate report".into());
    }
    Ok(())
}
