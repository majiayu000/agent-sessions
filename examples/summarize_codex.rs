//! A single-file Codex usage report. Run without arguments for synthetic data.
use agent_sessions::{Agent, Event, ReadOptions, read_from};
use serde_json::{Value, json};
use std::error::Error;
use std::fs::File;
use std::io::{BufRead, BufReader, Cursor};

const DEMO: &str = include_str!("fixtures/codex-session.jsonl");

fn summarize(source: impl BufRead) -> Result<Value, Box<dyn Error>> {
    let mut reader = read_from(Agent::Codex, source, &ReadOptions::default())?;
    let (mut messages, mut usage_events) = (0_u64, 0_u64);
    let (mut input, mut output) = (Some(0_u64), Some(0_u64));
    for event in reader.by_ref() {
        match event?.value {
            Event::Message(_) => messages += 1,
            Event::Usage(usage) => {
                usage_events += 1;
                input = add_known(input, usage.counts.input)?;
                output = add_known(output, usage.counts.output)?;
            }
            _ => {}
        }
    }
    let summary = reader.finish();
    if !summary.is_complete() || !summary.unknown_types.is_empty() {
        return Err("cannot report a complete total: incomplete or unknown records".into());
    }
    Ok(json!({
        "messages": messages,
        "usage_events": usage_events,
        "input_tokens": if usage_events > 0 { input } else { None },
        "output_tokens": if usage_events > 0 { output } else { None },
        "status": summary.status,
    }))
}

fn add_known(total: Option<u64>, next: Option<u64>) -> Result<Option<u64>, Box<dyn Error>> {
    match (total, next) {
        (Some(total), Some(next)) => Ok(Some(total.checked_add(next).ok_or("token overflow")?)),
        _ => Ok(None),
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args_os().skip(1);
    let path = args.next();
    if args.next().is_some() {
        return Err("usage: summarize_codex [session.jsonl]".into());
    }
    let report = match path {
        Some(path) => summarize(BufReader::new(File::open(path)?))?,
        None => summarize(Cursor::new(DEMO))?,
    };
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cumulative_updates_and_replays_do_not_double_count() {
        let report = summarize(Cursor::new(DEMO)).unwrap();
        assert_eq!(
            report,
            json!({"messages": 2, "usage_events": 2,
            "input_tokens": 150, "output_tokens": 30, "status": "Complete"})
        );
    }

    #[test]
    fn missing_usage_is_unknown_and_broken_records_fail() {
        assert!(summarize(Cursor::new("{}")).is_err());
        assert!(summarize(Cursor::new(format!("{DEMO}{{\"type\":"))).is_err());
        let report = summarize(Cursor::new("\n")).unwrap();
        assert!(report["input_tokens"].is_null());
        assert!(report["output_tokens"].is_null());
        assert_eq!(add_known(Some(5), None).unwrap(), None);
        assert!(add_known(Some(u64::MAX), Some(1)).is_err());
    }
}
