use super::{ReadStatus, SessionReader, TailMode, input::read_record};
use crate::parser::{parse, tally};
use crate::{Event, LineErrorKind, Located, Location, StreamError};
use std::io::BufRead;

impl<R: BufRead> Iterator for SessionReader<R> {
    type Item = Result<Located<Event>, StreamError>;
    fn next(&mut self) -> Option<Self::Item> {
        if let Some(event) = self.deliver() {
            return Some(Ok(event));
        }
        if self.ended {
            return None;
        }
        loop {
            let start = self.summary.bytes_read;
            let record = match read_record(&mut self.source, &self.opts, &mut self.summary) {
                Ok(Some(record)) => record,
                Ok(None) => {
                    self.ended = true;
                    self.summary.status = if self.summary.line_errors == 0 {
                        ReadStatus::Complete
                    } else {
                        ReadStatus::CompleteWithErrors
                    };
                    return None;
                }
                Err(error) => {
                    self.ended = true;
                    self.summary.status = ReadStatus::Failed;
                    return Some(Err(error));
                }
            };
            self.summary.lines += 1;
            let error = if record.too_long {
                Some(LineErrorKind::TooLong)
            } else if std::str::from_utf8(&record.bytes).is_err() {
                Some(LineErrorKind::InvalidUtf8)
            } else {
                None
            };
            if let Some(kind) = error {
                return Some(Err(self.line_error(start, kind)));
            }
            if record.bytes.iter().all(u8::is_ascii_whitespace) {
                self.checkpoint_record();
                continue;
            }
            let value = match serde_json::from_slice(&record.bytes) {
                Ok(value) => value,
                Err(e)
                    if !record.newline
                        && e.is_eof()
                        && self.opts.tail == TailMode::AllowIncomplete =>
                {
                    self.ended = true;
                    self.summary.truncated_tail = true;
                    self.summary.status = ReadStatus::IncompleteTail;
                    return None;
                }
                Err(_) => return Some(Err(self.line_error(start, LineErrorKind::InvalidJson))),
            };
            let parsed = match parse(
                self.agent,
                &value,
                &mut self.state,
                self.opts.codex_usage,
                self.opts.include,
                self.opts.accounting,
            ) {
                Ok(p) => p,
                Err(kind) => return Some(Err(self.line_error(start, kind))),
            };
            for unknown in &parsed.unknown {
                tally(&mut self.summary.unknown_types, unknown);
            }
            for ignored in &parsed.ignored {
                tally(&mut self.summary.ignored_types, ignored);
            }
            for (event_index, mut event) in parsed.events {
                if let Event::Message(m) = &mut event {
                    m.is_sidechain |= self.state.sidechain || self.file_sidechain;
                }
                if !self.opts.include.includes(&event) {
                    continue;
                }
                self.pending.push_back(Located {
                    location: Location {
                        record_index: self.summary.lines - 1,
                        line_no: self.summary.lines,
                        byte_start: start,
                        byte_end: self.summary.bytes_read,
                        event_index,
                    },
                    at: parsed.at,
                    timestamp_text: parsed.timestamp_text.clone(),
                    record_id: parsed.record_id.clone(),
                    session_id: self.state.session_id.clone(),
                    message_id: parsed.message_id.clone(),
                    value: event,
                });
            }
            self.checkpoint_record();
            if let Some(event) = self.deliver() {
                return Some(Ok(event));
            }
        }
    }
}

impl<R: BufRead> SessionReader<R> {
    fn checkpoint_record(&mut self) {
        if self.summary.line_errors == 0 {
            self.checkpoint = Some(self.summary.bytes_read);
            if self.pending.is_empty() {
                self.summary.last_complete_byte = self.summary.bytes_read;
                self.checkpoint = None;
            }
        }
    }
    fn line_error(&mut self, byte_start: u64, kind: LineErrorKind) -> StreamError {
        self.summary.line_errors += 1;
        if !matches!(
            kind,
            LineErrorKind::CounterRegression | LineErrorKind::LostUsageBaseline
        ) {
            self.state.broken_usage = true;
        }
        StreamError::Line {
            line_no: self.summary.lines,
            byte_start,
            kind,
        }
    }
}
