use crate::{Agent, DiscoverError, DiscoverOperation, FileKind, Roots};
use std::{
    fs, io,
    path::{Path, PathBuf},
    time::SystemTime,
};

#[derive(Debug, Clone, Default)]
pub struct DiscoverFilter {
    pub agents: Vec<Agent>,
    pub include_subagents: bool,
    pub modified_after: Option<SystemTime>,
}
#[derive(Debug, Clone)]
pub struct SessionFile {
    pub agent: Agent,
    pub path: PathBuf,
    pub kind: FileKind,
    pub size: u64,
    pub modified: SystemTime,
}
#[derive(Debug, Default)]
pub struct Discovery {
    pub files: Vec<SessionFile>,
    pub errors: Vec<DiscoverError>,
}

impl SessionFile {
    /// Explicit host selection for hooks and nonstandard roots.
    pub fn inspect(agent: Agent, path: impl Into<PathBuf>) -> io::Result<Self> {
        let path = path.into();
        let meta = fs::symlink_metadata(&path)?;
        if !meta.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "session path is not a regular file",
            ));
        }
        let kind = if path.components().any(|p| p.as_os_str() == "subagents") {
            FileKind::Subagent
        } else {
            FileKind::Main
        };
        Ok(Self {
            agent,
            path,
            kind,
            size: meta.len(),
            modified: meta.modified()?,
        })
    }
}

fn scan_roots(roots: &Roots) -> Vec<(Agent, PathBuf)> {
    let mut result = Vec::new();
    if let Some(root) = &roots.claude {
        result.push((Agent::ClaudeCode, root.join("projects")));
    }
    if let Some(root) = &roots.codex {
        for subdir in ["sessions", "archived_sessions"] {
            result.push((Agent::Codex, root.join(subdir)));
        }
    }
    result
}

/// Classify a known path lexically under configured roots, preserving IO errors.
/// Paths containing parent components are rejected rather than escaping a root.
pub fn classify(path: &Path, roots: &Roots) -> io::Result<Option<SessionFile>> {
    if path
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "parent path component",
        ));
    }
    if path.extension().is_none_or(|e| e != "jsonl") {
        return Ok(None);
    }
    for (agent, root) in scan_roots(roots) {
        if path.starts_with(root) {
            return SessionFile::inspect(agent, path).map(Some);
        }
    }
    Ok(None)
}

pub fn discover(roots: &Roots, filter: &DiscoverFilter) -> Discovery {
    let mut result = Discovery::default();
    for (agent, root) in scan_roots(roots) {
        scan_directory(agent, &root, filter, true, &mut result);
    }
    sort_files(&mut result);
    result
}

/// Scan an explicitly selected directory recursively. Unlike configured default
/// roots, a missing directory is reported in `errors`. Symlinks are not followed.
/// Subagent exclusion applies to descendant directories named `subagents`;
/// an explicitly selected root remains eligible regardless of its ancestors.
pub fn discover_directory(agent: Agent, root: &Path, filter: &DiscoverFilter) -> Discovery {
    let mut result = Discovery::default();
    scan_directory(agent, root, filter, false, &mut result);
    sort_files(&mut result);
    result
}

fn scan_directory(
    agent: Agent,
    root: &Path,
    filter: &DiscoverFilter,
    optional: bool,
    result: &mut Discovery,
) {
    if !filter.agents.is_empty() && !filter.agents.contains(&agent) {
        return;
    }
    let skip_tagged_subagents = optional && !filter.include_subagents;
    let mut pending = vec![(root.to_path_buf(), optional)];
    while let Some((dir, optional)) = pending.pop() {
        let metadata = match fs::symlink_metadata(&dir) {
            Ok(m) => m,
            Err(e) if optional && e.kind() == io::ErrorKind::NotFound => continue,
            Err(source) => {
                result.errors.push(DiscoverError {
                    agent,
                    path: dir,
                    source,
                    operation: DiscoverOperation::DirectoryMetadata,
                });
                continue;
            }
        };
        if metadata.is_symlink() {
            continue;
        }
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(source) => {
                result.errors.push(DiscoverError {
                    agent,
                    path: dir,
                    source,
                    operation: DiscoverOperation::ReadDirectory,
                });
                continue;
            }
        };
        for entry in entries {
            let entry = match entry {
                Ok(e) => e,
                Err(source) => {
                    result.errors.push(DiscoverError {
                        agent,
                        path: dir.clone(),
                        source,
                        operation: DiscoverOperation::ReadEntry,
                    });
                    continue;
                }
            };
            let path = entry.path();
            let ty = match entry.file_type() {
                Ok(ty) => ty,
                Err(source) => {
                    result.errors.push(DiscoverError {
                        agent,
                        path,
                        source,
                        operation: DiscoverOperation::FileType,
                    });
                    continue;
                }
            };
            if ty.is_dir() {
                if filter.include_subagents || entry.file_name() != "subagents" {
                    pending.push((path, false));
                }
            } else if ty.is_file() && path.extension().is_some_and(|e| e == "jsonl") {
                match SessionFile::inspect(agent, &path) {
                    Ok(file)
                        if filter.modified_after.is_none_or(|t| file.modified >= t)
                            && (!skip_tagged_subagents || file.kind == FileKind::Main) =>
                    {
                        result.files.push(file)
                    }
                    Ok(_) => {}
                    Err(source) => result.errors.push(DiscoverError {
                        agent,
                        path,
                        source,
                        operation: DiscoverOperation::InspectFile,
                    }),
                }
            }
        }
    }
}
fn sort_files(result: &mut Discovery) {
    result.files.sort_by(|a, b| a.path.cmp(&b.path));
    result
        .files
        .dedup_by(|a, b| a.path == b.path && a.agent == b.agent);
}
