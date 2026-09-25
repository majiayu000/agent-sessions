//! Read-only differential check against remem 7f4e144f's raw text projection.
//! This is a compatibility oracle, not a production adapter or DB migration.
//! Output contains counts only; never messages, paths, IDs or timestamps.
use agent_sessions::*;
use serde_json::Value;
use std::{
    fs::File,
    io::{BufRead, BufReader, Read},
    time::Instant,
};

type Projection = (u64, String, String, Option<i64>);

// Mirrors src/memory/raw_transcript.rs at the pinned reference. Intentionally
// independent of the library's text and timestamp helpers.
fn reference(v: &Value, ordinal: u64) -> Option<Projection> {
    let (role, content) = match v.get("type")?.as_str()? {
        "user" => ("user", &v["message"]["content"]),
        "assistant" => ("assistant", &v["message"]["content"]),
        "response_item" if v["payload"]["type"] == "message" => {
            let role = v["payload"]["role"].as_str()?;
            if role != "user" && role != "assistant" {
                return None;
            }
            (role, &v["payload"]["content"])
        }
        _ => return None,
    };
    let text = if let Some(a) = content.as_array() {
        a.iter()
            .filter_map(|b| match b["type"].as_str()? {
                "text" | "input_text" | "output_text" => b["text"].as_str(),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        content.as_str().unwrap_or("").to_owned()
    };
    let at = v
        .get("timestamp")
        .or_else(|| v.get("created_at"))
        .or_else(|| v.get("createdAt"))
        .or_else(|| v.pointer("/payload/timestamp"))
        .and_then(|t| {
            t.as_i64()
                .or_else(|| t.as_str()?.trim().parse::<i64>().ok())
                .or_else(|| {
                    chrono::DateTime::parse_from_rfc3339(t.as_str()?.trim())
                        .ok()
                        .map(|t| t.timestamp())
                })
        });
    Some((ordinal, role.into(), text, at))
}

fn next_reference(
    reader: &mut impl BufRead,
    ordinal: &mut u64,
) -> Result<Option<Projection>, Box<dyn std::error::Error>> {
    let mut line = String::new();
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            return Ok(None);
        }
        let index = *ordinal;
        *ordinal += 1;
        if line.trim().is_empty() {
            continue;
        }
        let v: Value = serde_json::from_str(&line)?;
        if let Some(p) = reference(&v, index) {
            return Ok(Some(p));
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let start = Instant::now();
    let files = discover(
        &Roots::from_env()?,
        &DiscoverFilter {
            include_subagents: true,
            ..Default::default()
        },
    );
    let mut compared = 0;
    let mut messages = 0;
    let mut different = 0;
    let mut incomplete = 0;
    for file in &files.files {
        let source = File::open(&file.path)?;
        let size = source.metadata()?.len();
        let mut old = BufReader::new(source.take(size));
        let mut ordinal = 0;
        let opts = ReadOptions {
            include: EventKinds::MESSAGE,
            stop_at_byte: Some(size),
            max_line_bytes: Some(16 * 1024 * 1024),
            tail: TailMode::Strict,
            ..Default::default()
        };
        let mut new = read(file, &opts)?;
        let mut mismatch = false;
        let mut failed = false;
        let mut count = 0;
        for event in new.by_ref() {
            let Ok(event) = event else {
                failed = true;
                continue;
            };
            let Event::Message(m) = event.value else {
                continue;
            };
            let role = match m.role {
                Role::User => "user",
                Role::Assistant => "assistant",
                _ => continue,
            };
            let value = (
                event.location.record_index,
                role.to_owned(),
                m.text,
                event.at.map(|t| t.timestamp()),
            );
            match next_reference(&mut old, &mut ordinal) {
                Ok(Some(reference)) => {
                    mismatch |= value != reference;
                    count += 1;
                }
                Ok(None) => mismatch = true,
                Err(_) => failed = true,
            }
        }
        match next_reference(&mut old, &mut ordinal) {
            Ok(Some(_)) => mismatch = true,
            Err(_) => failed = true,
            Ok(None) => {}
        }
        if failed || !new.finish().is_complete() {
            incomplete += 1;
        } else {
            compared += 1;
            messages += count;
            if mismatch {
                different += 1;
            }
        }
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "reference":"remem 7f4e144f raw_transcript text projection",
            "compared_files":compared,"compared_messages":messages,"different_files":different,
            "incomplete_files":incomplete,"discovery_errors":files.errors.len(),
            "elapsed_ms":start.elapsed().as_millis()
        }))?
    );
    if different > 0 || incomplete > 0 || !files.errors.is_empty() {
        return Err("comparison has differences or incomplete files".into());
    }
    Ok(())
}
