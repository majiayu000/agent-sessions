//! OpenCode 1.0's native storage/session, storage/message and storage/part tree.
use super::*;
use crate::parser::{required, string};
use std::path::PathBuf;

fn invalid(key: &str) -> ImportError {
    LineErrorKind::InvalidField(key.into()).into()
}
fn id(v: &Value, key: &'static str) -> Result<String, ImportError> {
    let id = required(v, key)?;
    if id.is_empty() || id == "." || id == ".." || id.contains(['/', '\\']) {
        return Err(invalid(key));
    }
    Ok(id.into())
}
fn load(path: &Path, opts: &ReadOptions, result: &mut SessionImport) -> Result<Value, ImportError> {
    let remaining = opts
        .max_file_bytes
        .map(|n| n.saturating_sub(result.summary.bytes_read));
    let file = File::open(path).map_err(ReadError::Io)?;
    let mut raw = read_raw_from(
        BufReader::new(file),
        &RawReadOptions {
            max_read_bytes: remaining,
            max_line_bytes: opts.max_line_bytes,
            ..Default::default()
        },
    )?;
    let mut bytes = Vec::new();
    for record in raw.by_ref() {
        bytes.extend_from_slice(&record?.bytes);
    }
    let summary = raw.finish();
    result.summary.bytes_read = result.summary.bytes_read.saturating_add(summary.bytes_read);
    result.summary.lines = result.summary.lines.saturating_add(summary.records);
    serde_json::from_slice(&bytes).map_err(|_| LineErrorKind::InvalidJson.into())
}
fn files(dir: &Path) -> Result<Vec<PathBuf>, ImportError> {
    let mut paths = Vec::new();
    // Empty sessions/messages legitimately have no directory yet.
    match std::fs::symlink_metadata(dir) {
        Ok(meta) if meta.file_type().is_symlink() => {
            return Err(invalid("OpenCode symlink directory"));
        }
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(paths),
        Err(e) => return Err(ReadError::Io(e).into()),
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(v) => v,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(paths),
        Err(e) => return Err(ReadError::Io(e).into()),
    };
    for entry in entries {
        let entry = entry.map_err(ReadError::Io)?;
        if entry.file_type().map_err(ReadError::Io)?.is_file()
            && entry.path().extension().is_some_and(|v| v == "json")
        {
            paths.push(entry.path());
        }
    }
    paths.sort();
    Ok(paths)
}
fn source(path: &Path) -> ImportSource {
    ImportSource::File {
        path: path.into(),
        pointer: String::new(),
    }
}

pub(super) fn import(path: &Path, opts: &ReadOptions) -> Result<SessionImport, ImportError> {
    opts.validate()?;
    if opts.stop_at_byte.is_some() {
        return Err(ReadError::InvalidOptions(
            "multi-file snapshots do not have one byte-prefix boundary",
        )
        .into());
    }
    let storage = path
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .ok_or_else(|| invalid("OpenCode storage path"))?;
    let mut result = SessionImport::default();
    let info = load(path, opts, &mut result)?;
    let session = id(&info, "id")?;
    let mut state = State {
        session_id: Some(session.clone()),
        sidechain: info.get("parentID").is_some_and(|v| !v.is_null()),
        ..Default::default()
    };
    emit_meta(
        MetaUpdate {
            session_id: Some(session.clone()),
            cwd: string(&info, "directory"),
            ..Default::default()
        },
        vec![source(path)],
        opts,
        &mut result,
    );
    let mut messages = Vec::new();
    for message_path in files(&storage.join("message").join(&session))? {
        let info = load(&message_path, opts, &mut result)?;
        let message_id = id(&info, "id")?;
        if info["sessionID"] != session {
            return Err(invalid("OpenCode message sessionID"));
        }
        let mut sources = vec![source(path), source(&message_path)];
        let mut parts = Vec::new();
        for part_path in files(&storage.join("part").join(&message_id))? {
            let part = load(&part_path, opts, &mut result)?;
            id(&part, "id")?;
            if part["messageID"] != message_id || part["sessionID"] != session {
                return Err(invalid("OpenCode part identity"));
            }
            parts.push(part);
            sources.push(source(&part_path));
        }
        parts.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
        messages.push((
            message_id,
            serde_json::json!({"info":info,"parts":parts}),
            sources,
        ));
    }
    messages.sort_by(|a, b| a.0.cmp(&b.0));
    for (_, row, sources) in messages {
        parse_record(
            Agent::OpenCode,
            &row,
            &mut state,
            sources,
            opts,
            &mut result,
        )?;
    }
    // File paths, not a fictional byte offset spanning independent JSON files.
    result.summary.last_complete_byte = 0;
    result.summary.status = ReadStatus::Complete;
    Ok(result)
}
