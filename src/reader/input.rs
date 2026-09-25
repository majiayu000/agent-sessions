use super::{ReadOptions, ReadSummary};
use crate::StreamError;
use std::io::BufRead;

pub(crate) struct Record {
    pub bytes: Vec<u8>,
    pub too_long: bool,
    pub newline: bool,
}

pub(crate) fn read_record<R: BufRead>(
    reader: &mut R,
    opts: &ReadOptions,
    summary: &mut ReadSummary,
    mut bytes: Vec<u8>,
) -> Result<Option<Record>, StreamError> {
    let start = summary.bytes_read;
    bytes.clear();
    let mut too_long = false;
    let mut newline = false;
    loop {
        if opts.stop_at_byte == Some(summary.bytes_read) {
            break;
        }
        let buf = reader.fill_buf().map_err(StreamError::Io)?;
        if buf.is_empty() {
            if let Some(expected) = opts.stop_at_byte
                && summary.bytes_read < expected
            {
                return Err(StreamError::SnapshotTruncated {
                    expected,
                    actual: summary.bytes_read,
                });
            }
            break;
        }
        if let Some(limit) = opts.max_file_bytes
            && summary.bytes_read >= limit
        {
            return Err(StreamError::TooLarge { limit });
        }
        let remaining = opts
            .stop_at_byte
            .unwrap_or(u64::MAX)
            .saturating_sub(summary.bytes_read)
            .min(
                opts.max_file_bytes
                    .unwrap_or(u64::MAX)
                    .saturating_sub(summary.bytes_read),
            );
        let available = buf
            .len()
            .min(usize::try_from(remaining).unwrap_or(usize::MAX));
        let slice = &buf[..available];
        let n = memchr::memchr(b'\n', slice).map_or(available, |i| i + 1);
        newline = slice.get(n.saturating_sub(1)) == Some(&b'\n');
        if !too_long {
            if opts
                .max_line_bytes
                .is_some_and(|limit| n > limit.saturating_sub(bytes.len()))
            {
                too_long = true;
                bytes.clear();
            } else {
                bytes.extend_from_slice(&slice[..n]);
            }
        }
        reader.consume(n);
        summary.bytes_read += n as u64;
        if newline {
            break;
        }
    }
    if summary.bytes_read == start {
        Ok(None)
    } else {
        Ok(Some(Record {
            bytes,
            too_long,
            newline,
        }))
    }
}
