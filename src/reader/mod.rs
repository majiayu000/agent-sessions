mod input;
mod options;
mod stream;
use crate::parser::State;
use crate::{Agent, Event, FileKind, Located, ReadError, SessionFile};
pub use options::*;
use std::{
    collections::VecDeque,
    fs::File,
    io::{BufRead, BufReader},
};

/// Stateful forward-only reader. Always inspect finish() after consuming events.
pub struct SessionReader<R = BufReader<File>> {
    source: R,
    agent: Agent,
    opts: ReadOptions,
    state: State,
    summary: ReadSummary,
    pending: VecDeque<Located<Event>>,
    ended: bool,
    checkpoint: Option<u64>,
    file_sidechain: bool,
}

pub fn read(file: &SessionFile, opts: &ReadOptions) -> Result<SessionReader, ReadError> {
    opts.validate()?;
    let source = File::open(&file.path).map_err(ReadError::Io)?;
    let metadata = source.metadata().map_err(ReadError::Io)?;
    if let Some(limit) = opts.max_file_bytes
        && metadata.len() > limit
    {
        return Err(ReadError::TooLarge { limit });
    }
    let mut reader = read_from(file.agent, BufReader::new(source), opts)?;
    reader.file_sidechain = file.kind == FileKind::Subagent;
    Ok(reader)
}

/// The supplied reader must start at record zero. Cumulative-only usage needs
/// the whole prefix; this API does not infer a baseline from an arbitrary seek.
pub fn read_from<R: BufRead>(
    agent: Agent,
    source: R,
    opts: &ReadOptions,
) -> Result<SessionReader<R>, ReadError> {
    opts.validate()?;
    Ok(SessionReader {
        source,
        agent,
        opts: opts.clone(),
        state: State::default(),
        summary: ReadSummary::default(),
        pending: VecDeque::new(),
        ended: false,
        checkpoint: None,
        file_sidechain: false,
    })
}

impl<R: BufRead> SessionReader<R> {
    /// Does not drain unread data. StoppedEarly is retained unless next() reached
    /// EOF or yielded a fatal error / explicit incomplete-tail result.
    pub fn finish(self) -> ReadSummary {
        self.summary
    }
    pub fn summary(&self) -> &ReadSummary {
        &self.summary
    }
    fn deliver(&mut self) -> Option<Located<Event>> {
        let event = self.pending.pop_front();
        if self.pending.is_empty()
            && let Some(end) = self.checkpoint.take()
        {
            self.summary.last_complete_byte = end;
        }
        event
    }
}
