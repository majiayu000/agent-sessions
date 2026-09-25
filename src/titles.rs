//! Explicit index metadata; never falls back to prompt/response bodies.
use crate::{Agent, Roots};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fs::{self, File},
    io::{self, BufRead, BufReader},
    path::Path,
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

pub fn load_session_titles(
    agent: Agent,
    roots: &Roots,
    ids: &[String],
) -> io::Result<HashMap<String, SessionTitle>> {
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let ids = ids.iter().map(String::as_str).collect::<HashSet<_>>();
    match agent {
        Agent::ClaudeCode => roots.claude.as_ref().map_or_else(
            || Ok(HashMap::new()),
            |root| claude(&root.join("projects"), &ids),
        ),
        Agent::Codex => roots.codex.as_ref().map_or_else(
            || Ok(HashMap::new()),
            |root| codex(&root.join("session_index.jsonl"), &ids),
        ),
    }
}
fn open(path: &Path) -> io::Result<Option<File>> {
    match File::open(path) {
        Ok(f) => Ok(Some(f)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}
fn invalid(path: &Path, error: serde_json::Error) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!(
            "malformed title index {} at line {}, column {}",
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
fn codex(path: &Path, ids: &HashSet<&str>) -> io::Result<HashMap<String, SessionTitle>> {
    let mut out = HashMap::new();
    let Some(file) = open(path)? else {
        return Ok(out);
    };
    for (index, line) in BufReader::new(file).lines().enumerate() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let row: CodexTitle = serde_json::from_str(&line).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "malformed title index {} at line {}",
                    path.display(),
                    index + 1
                ),
            )
        })?;
        if ids.contains(row.id.as_str()) && !row.thread_name.trim().is_empty() {
            out.insert(
                row.id,
                SessionTitle {
                    text: row.thread_name.trim().into(),
                    origin: SessionTitleOrigin::SourceTitle,
                },
            );
        }
    }
    Ok(out)
}
#[derive(Deserialize)]
struct ClaudeIndex {
    entries: Vec<ClaudeTitle>,
}
#[derive(Deserialize)]
struct ClaudeTitle {
    #[serde(rename = "sessionId")]
    session_id: String,
    summary: Option<String>,
}
fn claude(projects: &Path, ids: &HashSet<&str>) -> io::Result<HashMap<String, SessionTitle>> {
    let mut out = HashMap::new();
    let dirs = match fs::read_dir(projects) {
        Ok(d) => d,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(out),
        Err(e) => return Err(e),
    };
    for dir in dirs {
        let dir = dir?;
        if !dir.file_type()?.is_dir() {
            continue;
        }
        let path = dir.path().join("sessions-index.json");
        let Some(file) = open(&path)? else {
            continue;
        };
        let index: ClaudeIndex =
            serde_json::from_reader(BufReader::new(file)).map_err(|e| invalid(&path, e))?;
        for row in index.entries {
            if ids.contains(row.session_id.as_str())
                && let Some(s) = row.summary.filter(|s| !s.trim().is_empty())
            {
                out.insert(
                    row.session_id,
                    SessionTitle {
                        text: s.trim().into(),
                        origin: SessionTitleOrigin::SourceSummary,
                    },
                );
            }
        }
    }
    Ok(out)
}
