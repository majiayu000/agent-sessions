mod entry;
use crate::*;
pub use entry::*;
use serde_json::Value;
use std::{
    fs::File,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
};

#[derive(Debug, Clone)]
pub struct HistoryOptions {
    pub read: RawReadOptions,
    pub tail: TailMode,
    /// False preserves legacy record-counting behavior for valid JSON while
    /// exposing invalid optional fields in HistoryEntry.invalid_fields.
    pub strict_fields: bool,
}
impl Default for HistoryOptions {
    fn default() -> Self {
        Self {
            read: RawReadOptions::default(),
            tail: TailMode::Strict,
            strict_fields: true,
        }
    }
}
pub fn history_files(roots: &Roots) -> Vec<(Agent, PathBuf)> {
    [
        (Agent::ClaudeCode, &roots.claude),
        (Agent::Codex, &roots.codex),
    ]
    .into_iter()
    .filter_map(|(agent, root)| root.as_ref().map(|p| (agent, p.join("history.jsonl"))))
    .collect()
}
pub struct HistoryReader<R> {
    raw: RawReader<R>,
    agent: Agent,
    options: HistoryOptions,
    errors: u64,
    checkpoint: u64,
    incomplete: bool,
    ended: bool,
}
pub fn read_history(
    agent: Agent,
    path: &Path,
    options: &HistoryOptions,
) -> Result<HistoryReader<BufReader<File>>, ReadError> {
    Ok(HistoryReader::new(
        agent,
        read_raw_file(path, &options.read)?,
        options,
    ))
}
pub fn read_history_from<R: BufRead>(
    agent: Agent,
    source: R,
    options: &HistoryOptions,
) -> Result<HistoryReader<R>, ReadError> {
    Ok(HistoryReader::new(
        agent,
        read_raw_from(source, &options.read)?,
        options,
    ))
}
impl<R: BufRead> HistoryReader<R> {
    fn new(agent: Agent, raw: RawReader<R>, options: &HistoryOptions) -> Self {
        Self {
            raw,
            agent,
            options: options.clone(),
            errors: 0,
            checkpoint: options.read.start_offset,
            incomplete: false,
            ended: false,
        }
    }
    pub fn finish(self) -> ReadSummary {
        let raw = self.raw.finish();
        let status = if self.incomplete {
            ReadStatus::IncompleteTail
        } else if raw.status == ReadStatus::Complete && self.errors > 0 {
            ReadStatus::CompleteWithErrors
        } else {
            raw.status
        };
        ReadSummary {
            status,
            lines: raw.records,
            bytes_read: raw.bytes_read,
            last_complete_byte: self.checkpoint,
            truncated_tail: self.incomplete,
            line_errors: self.errors,
            ..Default::default()
        }
    }
}
impl<R: BufRead> Iterator for HistoryReader<R> {
    type Item = Result<Located<HistoryEntry>, StreamError>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.ended {
            return None;
        }
        loop {
            let raw = match self.raw.next()? {
                Ok(r) => r,
                Err(e) => {
                    if matches!(e, StreamError::Line { .. }) {
                        self.errors += 1;
                    }
                    return Some(Err(e));
                }
            };
            if raw.bytes.iter().all(u8::is_ascii_whitespace) {
                if self.errors == 0 {
                    self.checkpoint = raw.byte_end;
                }
                continue;
            }
            let entry = match serde_json::from_slice::<Value>(&raw.bytes) {
                Ok(v) => history_entry(self.agent, &v, self.options.strict_fields),
                Err(e)
                    if !raw.terminated
                        && e.is_eof()
                        && self.options.tail == TailMode::AllowIncomplete =>
                {
                    self.ended = true;
                    self.incomplete = true;
                    return None;
                }
                Err(_) => Err(LineErrorKind::InvalidJson),
            };
            return Some(match entry {
                Ok(entry) => {
                    if self.errors == 0 {
                        self.checkpoint = raw.byte_end;
                    }
                    Ok(Located {
                        location: Location {
                            record_index: raw.line_no - 1,
                            line_no: raw.line_no,
                            byte_start: raw.byte_start,
                            byte_end: raw.byte_end,
                            event_index: 0,
                        },
                        at: entry.at,
                        timestamp_text: None,
                        record_id: None,
                        session_id: entry.session_id.clone(),
                        message_id: None,
                        value: entry,
                    })
                }
                Err(kind) => {
                    self.errors += 1;
                    Err(StreamError::Line {
                        line_no: raw.line_no,
                        byte_start: raw.byte_start,
                        kind,
                    })
                }
            });
        }
    }
}
