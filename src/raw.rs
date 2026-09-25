//! Exact JSONL framing without interpreting JSON or changing bytes.
use crate::reader::input::read_record;
use crate::{LineErrorKind, ReadError, ReadOptions, ReadStatus, ReadSummary, StreamError};
use std::{
    fs::File,
    io::{BufRead, BufReader, Seek, SeekFrom},
    path::Path,
};

#[derive(Debug, Clone)]
pub struct RawReadOptions {
    pub start_offset: u64,
    pub stop_at_byte: Option<u64>,
    pub max_read_bytes: Option<u64>,
    pub max_line_bytes: Option<usize>,
}
impl Default for RawReadOptions {
    fn default() -> Self {
        Self {
            start_offset: 0,
            stop_at_byte: None,
            max_read_bytes: Some(200 * 1024 * 1024),
            max_line_bytes: Some(8 * 1024 * 1024),
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawRecord {
    pub byte_start: u64,
    pub byte_end: u64,
    pub line_no: u64,
    /// Exact bytes, including CR/LF delimiter when present.
    pub bytes: Vec<u8>,
    pub terminated: bool,
}
#[derive(Debug, Clone)]
pub struct RawSummary {
    pub status: ReadStatus,
    pub records: u64,
    pub bytes_read: u64,
    /// End of the contiguous delivered raw prefix, not a JSON-validity promise.
    pub delivered_through: u64,
    pub line_errors: u64,
}
pub struct RawReader<R> {
    reader: R,
    options: ReadOptions,
    summary: ReadSummary,
    start: u64,
    ended: bool,
}
pub fn read_raw_file(
    path: &Path,
    options: &RawReadOptions,
) -> Result<RawReader<BufReader<File>>, ReadError> {
    let mut file = File::open(path).map_err(ReadError::Io)?;
    let mut options = options.clone();
    if options.stop_at_byte.is_none() {
        options.stop_at_byte = Some(file.metadata().map_err(ReadError::Io)?.len());
    }
    file.seek(SeekFrom::Start(options.start_offset))
        .map_err(ReadError::Io)?;
    read_raw_from(BufReader::new(file), &options)
}
/// `source` must already be positioned at options.start_offset.
pub fn read_raw_from<R: BufRead>(
    source: R,
    options: &RawReadOptions,
) -> Result<RawReader<R>, ReadError> {
    if options
        .stop_at_byte
        .is_some_and(|end| end < options.start_offset)
    {
        return Err(ReadError::InvalidOptions(
            "raw boundary precedes start offset",
        ));
    }
    let limit = options
        .max_read_bytes
        .map(|n| {
            options
                .start_offset
                .checked_add(n)
                .ok_or(ReadError::InvalidOptions(
                    "raw read budget overflows offset",
                ))
        })
        .transpose()?;
    Ok(RawReader {
        reader: source,
        options: ReadOptions {
            stop_at_byte: options.stop_at_byte,
            max_file_bytes: limit,
            max_line_bytes: options.max_line_bytes,
            ..Default::default()
        },
        summary: ReadSummary {
            bytes_read: options.start_offset,
            last_complete_byte: options.start_offset,
            ..Default::default()
        },
        start: options.start_offset,
        ended: false,
    })
}
impl<R: BufRead> Iterator for RawReader<R> {
    type Item = Result<RawRecord, StreamError>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.ended {
            return None;
        }
        let start = self.summary.bytes_read;
        match read_record(
            &mut self.reader,
            &self.options,
            &mut self.summary,
            Vec::new(),
        ) {
            Ok(Some(record)) => {
                self.summary.lines += 1;
                if record.too_long {
                    self.summary.line_errors += 1;
                    return Some(Err(StreamError::Line {
                        line_no: self.summary.lines,
                        byte_start: start,
                        kind: LineErrorKind::TooLong,
                    }));
                }
                if self.summary.line_errors == 0 {
                    self.summary.last_complete_byte = self.summary.bytes_read;
                }
                Some(Ok(RawRecord {
                    byte_start: start,
                    byte_end: self.summary.bytes_read,
                    line_no: self.summary.lines,
                    bytes: record.bytes,
                    terminated: record.newline,
                }))
            }
            Ok(None) => {
                self.ended = true;
                self.summary.status = if self.summary.line_errors == 0 {
                    ReadStatus::Complete
                } else {
                    ReadStatus::CompleteWithErrors
                };
                None
            }
            Err(e) => {
                self.ended = true;
                self.summary.status = ReadStatus::Failed;
                Some(Err(e))
            }
        }
    }
}
impl<R: BufRead> RawReader<R> {
    pub fn finish(self) -> RawSummary {
        RawSummary {
            status: self.summary.status,
            records: self.summary.lines,
            bytes_read: self.summary.bytes_read - self.start,
            delivered_through: self.summary.last_complete_byte,
            line_errors: self.summary.line_errors,
        }
    }
}
