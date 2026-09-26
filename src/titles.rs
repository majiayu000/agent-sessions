//! Explicit index metadata; never falls back to prompt/response bodies.
use crate::{Agent, RawReadOptions, Roots, StreamError, read_raw_from};
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;
use std::{
    collections::{HashMap, HashSet},
    fs::{self, File},
    io::{self, BufReader, Read},
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionTitleOrigin {
    SourceTitle,
    SourceSummary,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionTitle {
    pub text: String,
    pub origin: SessionTitleOrigin,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum TitleErrorMode {
    /// Stop at the first error, preserving the original title API's contract.
    #[default]
    Strict,
    /// Return valid titles together with errors. Results may be incomplete.
    Collect,
}

#[derive(Debug, Clone)]
pub struct TitleReadOptions {
    /// Per-index byte budget, checked before allocation. None disables it.
    pub max_file_bytes: Option<u64>,
    /// Codex JSONL record budget including the delimiter. Claude's JSON document
    /// is bounded by max_file_bytes, not by its physical line layout.
    pub max_line_bytes: Option<usize>,
    pub error_mode: TitleErrorMode,
}
impl Default for TitleReadOptions {
    fn default() -> Self {
        Self {
            max_file_bytes: Some(200 * 1024 * 1024),
            max_line_bytes: Some(8 * 1024 * 1024),
            error_mode: TitleErrorMode::Strict,
        }
    }
}

#[derive(Debug)]
pub struct TitleReadError {
    pub path: PathBuf,
    /// Physical Codex JSONL line; None for file errors or Claude JSON entries.
    pub line_no: Option<u64>,
    /// Syntax/schema errors contain locations, never title or field values.
    pub source: io::Error,
}

#[derive(Debug, Default)]
pub struct TitleReadResult {
    pub titles: HashMap<String, SessionTitle>,
    /// Nonempty means partial results, even if all requested IDs were found.
    pub errors: Vec<TitleReadError>,
}
impl TitleReadResult {
    fn record_error(
        &mut self,
        options: &TitleReadOptions,
        path: &Path,
        line_no: Option<u64>,
        source: io::Error,
    ) -> io::Result<()> {
        if options.error_mode == TitleErrorMode::Strict {
            return Err(source);
        }
        self.errors.push(TitleReadError {
            path: path.to_owned(),
            line_no,
            source,
        });
        Ok(())
    }
}

pub fn load_session_titles(
    agent: Agent,
    roots: &Roots,
    ids: &[String],
) -> io::Result<HashMap<String, SessionTitle>> {
    load_session_titles_with_options(agent, roots, ids, &TitleReadOptions::default())
        .map(|result| result.titles)
}

/// Read captured index prefixes with explicit limits and error policy. Collect
/// mode preserves valid titles across bad rows/files; malformed Claude document
/// syntax cannot be recovered within that file. Check errors before trusting
/// completeness. No transcript bodies are read or used as fallback titles.
pub fn load_session_titles_with_options(
    agent: Agent,
    roots: &Roots,
    ids: &[String],
    options: &TitleReadOptions,
) -> io::Result<TitleReadResult> {
    let mut result = TitleReadResult::default();
    if ids.is_empty() {
        return Ok(result);
    }
    let ids = ids.iter().map(String::as_str).collect::<HashSet<_>>();
    match agent {
        Agent::ClaudeCode => {
            if let Some(root) = &roots.claude {
                claude(&root.join("projects"), &ids, options, &mut result)?;
            }
        }
        Agent::Codex => {
            if let Some(root) = &roots.codex {
                let path = root.join("session_index.jsonl");
                if let Err(error) = codex(&path, &ids, options, &mut result) {
                    result.record_error(options, &path, None, error)?;
                }
            }
        }
    }
    Ok(result)
}

fn open(path: &Path, options: &TitleReadOptions) -> io::Result<Option<(File, u64)>> {
    let file = match File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    let size = file.metadata()?.len();
    if let Some(limit) = options.max_file_bytes
        && size > limit
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Title index {} exceeds {limit} byte limit", path.display()),
        ));
    }
    Ok(Some((file, size)))
}
fn invalid(path: &Path, error: serde_json::Error) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!(
            "Malformed title index {} at line {}, column {}",
            path.display(),
            error.line(),
            error.column()
        ),
    )
}
#[derive(Deserialize)]
struct CodexTitle {
    id: String,
    thread_name: String,
}
fn codex(
    path: &Path,
    ids: &HashSet<&str>,
    options: &TitleReadOptions,
    result: &mut TitleReadResult,
) -> io::Result<()> {
    let Some((file, size)) = open(path, options)? else {
        return Ok(());
    };
    let mut reader = read_raw_from(
        BufReader::new(file),
        &RawReadOptions {
            stop_at_byte: Some(size),
            max_read_bytes: options.max_file_bytes,
            max_line_bytes: options.max_line_bytes,
            ..Default::default()
        },
    )
    .map_err(io::Error::other)?;
    while let Some(record) = reader.next() {
        let record = match record {
            Ok(record) => record,
            Err(error) => {
                let line_no = match &error {
                    StreamError::Line { line_no, .. } => *line_no,
                    _ => reader.next_line_no(),
                };
                let source = match error {
                    StreamError::Io(error) => error,
                    StreamError::SnapshotTruncated { .. } => {
                        io::Error::new(io::ErrorKind::UnexpectedEof, error)
                    }
                    _ => io::Error::new(io::ErrorKind::InvalidData, error),
                };
                result.record_error(options, path, Some(line_no), source)?;
                continue;
            }
        };
        if std::str::from_utf8(&record.bytes).is_ok_and(|s| s.trim().is_empty()) {
            continue;
        }
        let row: CodexTitle = match serde_json::from_slice(&record.bytes) {
            Ok(row) => row,
            Err(_) => {
                result.record_error(
                    options,
                    path,
                    Some(record.line_no),
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!(
                            "Malformed title index {} at line {}",
                            path.display(),
                            record.line_no
                        ),
                    ),
                )?;
                continue;
            }
        };
        if ids.contains(row.id.as_str()) && !row.thread_name.trim().is_empty() {
            result.titles.insert(
                row.id,
                SessionTitle {
                    text: row.thread_name.trim().into(),
                    origin: SessionTitleOrigin::SourceTitle,
                },
            );
        }
    }
    Ok(())
}
#[derive(Deserialize)]
struct ClaudeIndex<'a> {
    #[serde(borrow)]
    entries: Vec<&'a RawValue>,
}
#[derive(Deserialize)]
struct ClaudeTitle {
    #[serde(rename = "sessionId")]
    session_id: String,
    summary: Option<String>,
}
fn claude_index(
    path: &Path,
    ids: &HashSet<&str>,
    options: &TitleReadOptions,
    result: &mut TitleReadResult,
) -> io::Result<()> {
    let Some((file, size)) = open(path, options)? else {
        return Ok(());
    };
    // Seal the length so concurrent appends cannot bypass the allocation budget.
    let mut bytes = Vec::new();
    file.take(size).read_to_end(&mut bytes)?;
    if bytes.len() as u64 != size {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "Title index snapshot was truncated",
        ));
    }
    let index: ClaudeIndex<'_> = serde_json::from_slice(&bytes).map_err(|e| invalid(path, e))?;
    for (entry_index, raw) in index.entries.into_iter().enumerate() {
        let row: ClaudeTitle = match serde_json::from_str(raw.get()) {
            Ok(row) => row,
            Err(_) => {
                result.record_error(
                    options,
                    path,
                    None,
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!(
                            "Malformed title index {} at entry {}",
                            path.display(),
                            entry_index + 1
                        ),
                    ),
                )?;
                continue;
            }
        };
        if ids.contains(row.session_id.as_str())
            && let Some(s) = row.summary.filter(|s| !s.trim().is_empty())
        {
            result.titles.insert(
                row.session_id,
                SessionTitle {
                    text: s.trim().into(),
                    origin: SessionTitleOrigin::SourceSummary,
                },
            );
        }
    }
    Ok(())
}
fn claude(
    projects: &Path,
    ids: &HashSet<&str>,
    options: &TitleReadOptions,
    result: &mut TitleReadResult,
) -> io::Result<()> {
    let dirs = match fs::read_dir(projects) {
        Ok(d) => d,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return result.record_error(options, projects, None, e),
    };
    for dir in dirs {
        let dir = match dir {
            Ok(dir) => dir,
            Err(e) => {
                result.record_error(options, projects, None, e)?;
                continue;
            }
        };
        let ty = match dir.file_type() {
            Ok(ty) => ty,
            Err(e) => {
                result.record_error(options, &dir.path(), None, e)?;
                continue;
            }
        };
        if !ty.is_dir() {
            continue;
        }
        let path = dir.path().join("sessions-index.json");
        if let Err(error) = claude_index(&path, ids, options, result) {
            result.record_error(options, &path, None, error)?;
        }
    }
    Ok(())
}
