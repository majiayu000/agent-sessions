use agent_sessions::*;
use std::io::{self, BufReader, Cursor, Read};

fn reader(s: &[u8], opts: ReadOptions) -> SessionReader<Cursor<&[u8]>> {
    read_from(Agent::ClaudeCode, Cursor::new(s), &opts).unwrap()
}
fn message() -> Vec<u8> {
    b"{\"type\":\"user\",\"message\":{\"content\":\" hello \"}}\n".to_vec()
}

#[test]
fn physical_occurrences_and_whitespace_are_preserved() {
    let mut data = vec![b'\n'];
    data.extend(message());
    data.extend(message());
    let mut r = reader(&data, ReadOptions::default());
    let events = r.by_ref().collect::<Result<Vec<_>, _>>().unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].location.record_index, 1);
    assert_eq!(events[1].location.record_index, 2);
    assert_eq!(events[0].location.byte_start, 1);
    assert_eq!(events[0].location.byte_end, 1 + message().len() as u64);
    assert!(matches!(&events[0].value,Event::Message(m) if m.text==" hello "));
    assert_eq!(r.finish().last_complete_byte, data.len() as u64);
}

#[test]
fn valid_final_record_needs_no_newline() {
    let mut data = message();
    data.pop();
    let mut r = reader(&data, ReadOptions::default());
    assert_eq!(r.by_ref().count(), 1);
    assert!(r.finish().is_complete());
}

#[test]
fn tail_requires_explicit_tolerance_and_syntax_eof() {
    for (tail, valid_partial) in [
        (b"{\"type\":".as_slice(), true),
        (b"{broken}".as_slice(), false),
    ] {
        let mut data = message();
        data.extend(tail);
        let mut r = reader(
            &data,
            ReadOptions {
                tail: TailMode::AllowIncomplete,
                ..Default::default()
            },
        );
        assert!(r.next().unwrap().is_ok());
        if valid_partial {
            assert!(r.next().is_none());
            assert_eq!(r.summary().status, ReadStatus::IncompleteTail);
        } else {
            assert!(r.next().unwrap().is_err());
            assert!(r.next().is_none());
        }
        assert_eq!(r.finish().last_complete_byte, message().len() as u64);
    }
    let mut r = reader(b"{", ReadOptions::default());
    assert!(r.next().unwrap().is_err());
    assert!(r.next().is_none());
    assert_eq!(r.finish().status, ReadStatus::CompleteWithErrors);
    let mut r = reader(
        b"{\n",
        ReadOptions {
            tail: TailMode::AllowIncomplete,
            ..Default::default()
        },
    );
    assert!(r.next().unwrap().is_err());
}

#[test]
fn malformed_middle_and_utf8_are_visible_and_do_not_advance_checkpoint() {
    let mut data = message();
    data.extend(b"oops\n\xff\n");
    data.extend(message());
    let mut r = reader(&data, ReadOptions::default());
    let out = r.by_ref().collect::<Vec<_>>();
    assert_eq!(out.len(), 4);
    assert!(out[0].is_ok());
    assert!(out[3].is_ok());
    assert!(matches!(
        &out[2],
        Err(StreamError::Line {
            kind: LineErrorKind::InvalidUtf8,
            ..
        })
    ));
    let s = r.finish();
    assert_eq!(s.line_errors, 2);
    assert_eq!(s.last_complete_byte, message().len() as u64);
    assert_eq!(s.status, ReadStatus::CompleteWithErrors);
}

#[test]
fn oversized_line_is_drained_before_next_record() {
    let mut data = vec![b'x'; 100_000];
    data.push(b'\n');
    data.extend(message());
    let mut r = reader(
        &data,
        ReadOptions {
            max_line_bytes: Some(128),
            ..Default::default()
        },
    );
    assert!(matches!(
        r.next(),
        Some(Err(StreamError::Line {
            kind: LineErrorKind::TooLong,
            ..
        }))
    ));
    assert_eq!(r.next().unwrap().unwrap().location.record_index, 1);
    assert!(r.next().is_none());
    assert_eq!(r.finish().last_complete_byte, 0);
}

#[test]
fn stream_file_limit_is_fatal_once() {
    let data = message();
    let limit = data.len() as u64 - 1;
    let mut r = reader(
        &data,
        ReadOptions {
            max_file_bytes: Some(limit),
            ..Default::default()
        },
    );
    assert!(matches!(r.next(), Some(Err(StreamError::TooLarge { .. }))));
    assert!(r.next().is_none());
    assert_eq!(r.finish().status, ReadStatus::Failed);
}

#[test]
fn exact_snapshot_and_short_snapshot() {
    let mut data = message();
    let boundary = data.len() as u64;
    data.extend(b"garbage");
    let opts = ReadOptions {
        stop_at_byte: Some(boundary),
        ..Default::default()
    };
    let mut r = reader(&data, opts.clone());
    assert_eq!(r.by_ref().count(), 1);
    assert!(r.finish().is_complete());
    let mut r = reader(b"{}", opts);
    assert!(matches!(
        r.next(),
        Some(Err(StreamError::SnapshotTruncated { .. }))
    ));
    assert!(r.next().is_none());
    assert_eq!(r.finish().status, ReadStatus::Failed);
}

#[test]
fn early_stop_is_not_completion_and_partial_record_is_not_checkpointed() {
    let data = br#"{"type":"assistant","cwd":"/work","message":{"content":"text"}}"#;
    let mut r = reader(data, ReadOptions::default());
    assert!(matches!(r.next().unwrap().unwrap().value, Event::Meta(_)));
    let s = r.finish();
    assert_eq!(s.status, ReadStatus::StoppedEarly);
    assert_eq!(s.last_complete_byte, 0);
    let r = reader(data, ReadOptions::default());
    assert_eq!(r.finish().bytes_read, 0);
}

struct Breaks {
    prefix: Cursor<Vec<u8>>,
}
impl Read for Breaks {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        let n = self.prefix.read(out)?;
        if n == 0 {
            Err(io::Error::other("synthetic IO failure"))
        } else {
            Ok(n)
        }
    }
}
#[test]
fn late_io_failure_preserves_failed_status() {
    let source = BufReader::with_capacity(
        3,
        Breaks {
            prefix: Cursor::new(message()),
        },
    );
    let mut r = read_from(Agent::ClaudeCode, source, &ReadOptions::default()).unwrap();
    assert!(r.next().unwrap().is_ok());
    assert!(matches!(r.next(), Some(Err(StreamError::Io(_)))));
    assert!(r.next().is_none());
    assert_eq!(r.finish().status, ReadStatus::Failed);
}
