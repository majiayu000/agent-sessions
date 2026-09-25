use crate::{CodexUsageMode, EventKinds, ReadError};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum TailMode {
    #[default]
    Strict,
    AllowIncomplete,
}

#[derive(Debug, Clone)]
pub struct ReadOptions {
    pub accounting: crate::AccountingPolicy,
    pub max_file_bytes: Option<u64>,
    /// Includes the line delimiter. Enforced before allocating the whole line.
    pub max_line_bytes: Option<usize>,
    pub stop_at_byte: Option<u64>,
    pub include: EventKinds,
    pub tail: TailMode,
    pub codex_usage: CodexUsageMode,
}
impl Default for ReadOptions {
    fn default() -> Self {
        Self {
            max_file_bytes: Some(200 * 1024 * 1024),
            max_line_bytes: Some(8 * 1024 * 1024),
            stop_at_byte: None,
            include: EventKinds::ALL,
            tail: TailMode::Strict,
            codex_usage: CodexUsageMode::TokenCount,
            accounting: crate::AccountingPolicy::Strict,
        }
    }
}
impl ReadOptions {
    pub(crate) fn validate(&self) -> Result<(), ReadError> {
        if matches!((self.stop_at_byte,self.max_file_bytes),(Some(s),Some(m)) if s>m) {
            return Err(ReadError::InvalidOptions("snapshot exceeds byte budget"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub enum ReadStatus {
    Complete,
    CompleteWithErrors,
    IncompleteTail,
    #[default]
    StoppedEarly,
    Failed,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ReadSummary {
    pub status: ReadStatus,
    pub lines: u64,
    pub bytes_read: u64,
    /// End of the contiguous, error-free, fully delivered record prefix.
    pub last_complete_byte: u64,
    pub truncated_tail: bool,
    pub line_errors: u64,
    pub unknown_types: BTreeMap<String, u64>,
    pub ignored_types: BTreeMap<String, u64>,
}
impl ReadSummary {
    pub fn is_complete(&self) -> bool {
        self.status == ReadStatus::Complete
    }
}
