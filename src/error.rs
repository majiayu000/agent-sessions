use serde::Serialize;
use std::{fmt, io, path::PathBuf};

#[derive(Debug)]
pub struct DiscoverError {
    pub path: PathBuf,
    pub source: io::Error,
}

#[derive(Debug)]
#[non_exhaustive]
pub enum ReadError {
    Io(io::Error),
    TooLarge { limit: u64 },
    InvalidOptions(&'static str),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub enum LineErrorKind {
    InvalidJson,
    InvalidUtf8,
    TooLong,
    MissingField(&'static str),
    InvalidField(String),
    CounterRegression,
    LostUsageBaseline,
}

#[derive(Debug)]
#[non_exhaustive]
pub enum StreamError {
    Line {
        line_no: u64,
        byte_start: u64,
        kind: LineErrorKind,
    },
    Io(io::Error),
    TooLarge {
        limit: u64,
    },
    SnapshotTruncated {
        expected: u64,
        actual: u64,
    },
}

impl fmt::Display for ReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(_) => write!(f, "cannot open session file"),
            Self::TooLarge { limit } => write!(f, "session exceeds {limit} byte limit"),
            Self::InvalidOptions(msg) => write!(f, "invalid read options: {msg}"),
        }
    }
}
impl std::error::Error for ReadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            _ => None,
        }
    }
}
impl fmt::Display for StreamError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Line { line_no, kind, .. } => write!(f, "session line {line_no}: {kind:?}"),
            Self::Io(_) => write!(f, "session read failed"),
            Self::TooLarge { limit } => write!(f, "session exceeds {limit} byte limit"),
            Self::SnapshotTruncated { expected, actual } => {
                write!(f, "snapshot requires {expected} bytes; read {actual}")
            }
        }
    }
}
impl std::error::Error for StreamError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            _ => None,
        }
    }
}
