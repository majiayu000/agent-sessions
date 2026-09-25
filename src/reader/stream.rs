use super::{ReadStatus, SessionReader, TailMode, input::read_record};
use crate::parser::tally;
use crate::statistics::{DecodeError, decode};
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
            let record = match read_record(
                &mut self.source,
                &self.opts,
                &mut self.summary,
                std::mem::take(&mut self.scratch),
            ) {
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
                self.scratch = record.bytes;
                return Some(Err(self.line_error(start, kind)));
            }
            if record.bytes.iter().all(u8::is_ascii_whitespace) {
                self.scratch = record.bytes;
                self.checkpoint_record();
                continue;
            }
            let decoded = decode(self.agent, &record.bytes, &mut self.state, &self.opts);
            self.scratch = record.bytes;
            let mut parsed = match decoded {
                Ok(parsed) => parsed,
                Err(DecodeError::Json(e))
                    if !record.newline
                        && e.is_eof()
                        && self.opts.tail == TailMode::AllowIncomplete =>
                {
                    self.ended = true;
                    self.summary.truncated_tail = true;
                    self.summary.status = ReadStatus::IncompleteTail;
                    return None;
                }
                Err(DecodeError::Json(_)) => {
                    return Some(Err(self.line_error(start, LineErrorKind::InvalidJson)));
                }
                Err(DecodeError::Fields(kind)) => return Some(Err(self.line_error(start, kind))),
            };
            if let Some((known, label)) = &parsed.ignored_one {
                if *known {
                    tally(&mut self.summary.ignored_types, label);
                } else {
                    tally(&mut self.summary.unknown_types, label);
                }
            }
            for unknown in &parsed.unknown {
                tally(&mut self.summary.unknown_types, unknown);
            }
            for ignored in &parsed.ignored {
                tally(&mut self.summary.ignored_types, ignored);
            }
            let event_count = parsed.events.len();
            for (ordinal, (event_index, mut event)) in parsed.events.into_iter().enumerate() {
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
                    timestamp_text: if ordinal + 1 == event_count {
                        parsed.timestamp_text.take()
                    } else {
                        parsed.timestamp_text.clone()
                    },
                    record_id: if ordinal + 1 == event_count {
                        parsed.record_id.take()
                    } else {
                        parsed.record_id.clone()
                    },
                    record_type: if ordinal + 1 == event_count {
                        parsed.record_type.take()
                    } else {
                        parsed.record_type.clone()
                    },
                    session_id: self.state.session_id.clone(),
                    message_id: if ordinal + 1 == event_count {
                        parsed.message_id.take()
                    } else {
                        parsed.message_id.clone()
                    },
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
