//! Live acceptance: real clients, their native persistence, and independent task expectations.
//! Opt in with `cargo test --test native_e2e -- --ignored --nocapture --test-threads=1`.
//! Uses existing client authentication/model settings and makes paid model requests.
use agent_sessions::{
    Agent, Event, ReadOptions, Role, SessionImport, ToolArgs, import_database, import_session,
};
use serde_json::Value;
use std::{
    error::Error,
    fs::{self, File, OpenOptions},
    io::{self, BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

type TestResult = Result<(), Box<dyn Error>>;

#[test]
#[ignore = "requires installed/authenticated Codex; starts a real model session"]
fn codex_native_e2e() -> TestResult {
    live(Agent::Codex)
}

#[test]
#[ignore = "requires installed/authenticated Grok Build; starts a real model session"]
fn grok_native_e2e() -> TestResult {
    live(Agent::Grok)
}

#[test]
#[ignore = "requires installed/authenticated OpenCode; starts a real model session"]
fn opencode_native_e2e() -> TestResult {
    live(Agent::OpenCode)
}

#[test]
#[ignore = "requires installed/authenticated Claude Code; starts a real model session"]
fn claude_native_e2e() -> TestResult {
    live(Agent::ClaudeCode)
}

#[test]
#[ignore = "requires installed/authenticated Kimi Code; starts a real model session"]
fn kimi_native_e2e() -> TestResult {
    live(Agent::KimiCli)
}

#[test]
#[ignore = "requires installed/authenticated Cline CLI; starts a real model session"]
fn cline_cli_native_e2e() -> TestResult {
    live(Agent::ClineCli)
}

#[test]
#[ignore = "requires installed/authenticated Hermes; starts a real model session"]
fn hermes_native_e2e() -> TestResult {
    live(Agent::Hermes)
}

#[test]
#[ignore = "requires installed/authenticated Cursor Agent CLI; starts a real model session"]
fn cursor_cli_native_e2e() -> TestResult {
    live(Agent::CursorCli)
}

#[test]
#[ignore = "requires installed/authenticated WorkBuddy; starts a real model session"]
fn workbuddy_native_e2e() -> TestResult {
    live(Agent::WorkBuddy)
}

fn live(agent: Agent) -> TestResult {
    let client = match agent {
        Agent::Codex => "codex",
        Agent::Grok => "grok",
        Agent::OpenCode => "opencode",
        Agent::ClaudeCode => "claude",
        Agent::KimiCli => "kimi",
        Agent::ClineCli => "cline",
        Agent::Hermes => "hermes",
        Agent::CursorCli => "cursor-agent",
        Agent::WorkBuddy => "workbuddy",
        _ => unreachable!(),
    };
    let executable = if agent == Agent::WorkBuddy && cfg!(target_os = "macos") {
        PathBuf::from(
            "/Applications/WorkBuddy.app/Contents/Resources/app.asar.unpacked/cli/bin/codebuddy",
        )
    } else {
        PathBuf::from(client)
    };
    // Keep private artifacts for independent inspection, including on failure.
    let dir = tempfile::Builder::new()
        .prefix(&format!("agent-sessions-e2e-{client}-"))
        .tempdir()?
        .keep();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))?;
    }
    eprintln!("{client}: private evidence directory {}", dir.display());
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let fixture_name = format!("input-{nonce:x}.txt");
    // The prompt contains the filename, never the answer. Expectations predate the model run.
    let expected = format!("agent-sessions-e2e-{nonce:x}\n中文：读取文件并保存结果\n");
    fs::write(dir.join(&fixture_name), &expected)?;
    fs::write(dir.join("expected.txt"), &expected)?;
    fs::write(
        dir.join("AGENTS.md"),
        "Only perform the requested acceptance task in this directory. Do not use subagents, network tools, or read other directories.\n",
    )?;
    let prompt = format!(
        "Acceptance task in the current directory: use a shell/terminal tool to run `cat {fixture_name}` \
         and `cp {fixture_name} result.txt`. The tool must return the complete cat output. \
         Finally reply with exactly the complete contents of the input file, with no commentary or Markdown. \
         Do not inspect any other files, use subagents, or access the network."
    );
    fs::write(dir.join("prompt.txt"), &prompt)?;
    run(Command::new(&executable).arg("--version"), &dir, "version")?;
    let mut command = Command::new(&executable);
    let grok_id = format!("00000000-0000-4000-8000-{:012x}", nonce & 0xffffffffffff);
    let cursor_id = if agent == Agent::CursorCli {
        run(
            Command::new(&executable)
                .arg("--workspace")
                .arg(&dir)
                .arg("create-chat"),
            &dir,
            "create-chat",
        )?;
        let id = fs::read_to_string(dir.join("create-chat.stdout"))?
            .trim()
            .to_owned();
        if id.is_empty()
            || !id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return Err("Cursor create-chat did not return a native chat id".into());
        }
        Some(id)
    } else {
        None
    };
    match agent {
        Agent::Codex => {
            command.arg("exec").arg("--cd").arg(&dir).args([
                "--skip-git-repo-check",
                "--sandbox",
                "workspace-write",
                "--json",
                &prompt,
            ]);
        }
        Agent::Grok => {
            command.arg("--cwd").arg(&dir).args([
                "--session-id",
                &grok_id,
                "--no-subagents",
                "--disable-web-search",
                "--permission-mode",
                "acceptEdits",
                "--allow",
                "Bash(cat *)",
                "--allow",
                "Bash(cp *)",
                "--output-format",
                "streaming-json",
                "--single",
                &prompt,
            ]);
        }
        Agent::OpenCode => {
            command
                .args(["run", "--pure", "--format", "json"])
                .arg("--dir")
                .arg(&dir)
                .arg(&prompt);
        }
        Agent::ClaudeCode => {
            command
                .args([
                    "--print",
                    "--verbose",
                    "--output-format",
                    "stream-json",
                    "--session-id",
                    &grok_id,
                    "--permission-mode",
                    "acceptEdits",
                    "--tools",
                    "Bash",
                    "--allowedTools",
                    "Bash(cat *)",
                    "Bash(cp *)",
                    "--strict-mcp-config",
                    "--disable-slash-commands",
                ])
                .arg(&prompt);
        }
        Agent::KimiCli => {
            command.args(["--output-format", "stream-json", "--prompt", &prompt]);
        }
        Agent::ClineCli => {
            command
                .arg("--cwd")
                .arg(&dir)
                .arg("--data-dir")
                .arg(dir.join("cline-data"))
                .args(["--json", "--timeout", "120", "--retries", "1"])
                .arg(&prompt);
        }
        Agent::Hermes => {
            command.args([
                "chat",
                "--toolsets",
                "terminal",
                "--max-turns",
                "8",
                "--query",
                &prompt,
            ]);
        }
        Agent::CursorCli => {
            command
                .arg("--workspace")
                .arg(&dir)
                .args([
                    "--resume",
                    cursor_id.as_deref().unwrap(),
                    "--print",
                    "--output-format",
                    "stream-json",
                    "--trust",
                    "--force",
                ])
                .arg(&prompt);
        }
        Agent::WorkBuddy => {
            command
                .args([
                    "--print",
                    "--verbose",
                    "--output-format",
                    "stream-json",
                    "--session-id",
                    &grok_id,
                    "--permission-mode",
                    "acceptEdits",
                    "--tools",
                    "Bash",
                    "--allowedTools",
                    "Bash(cat *)",
                    "Bash(cp *)",
                    "--strict-mcp-config",
                    "--max-turns",
                    "8",
                ])
                .arg(&prompt);
        }
        _ => unreachable!(),
    }
    run(&mut command, &dir, "client")?;
    // This checks the real filesystem side effect before invoking the library.
    let actual_file = fs::read_to_string(dir.join("result.txt")).map_err(|e| {
        format!("{client} did not produce a readable result.txt ({e}); inspect private client logs")
    })?;
    if actual_file != expected {
        return Err("client did not create the independently expected file contents".into());
    }
    let home = std::env::home_dir().ok_or("home directory unavailable")?;
    let (native, database_id) = match agent {
        Agent::Codex => {
            let id = stdout_id(&dir.join("client.stdout"), "thread_id")?;
            let root = agent_sessions::Roots::from_env_for(Agent::Codex)?
                .codex
                .ok_or("Codex storage root unavailable")?;
            (find_session(&root.join("sessions"), &id, false)?, None)
        }
        Agent::Grok => {
            let root = std::env::home_dir().ok_or("home directory unavailable")?;
            (
                find_session(&root.join(".grok/sessions"), &grok_id, true)?,
                None,
            )
        }
        Agent::OpenCode => {
            let id = stdout_id(&dir.join("client.stdout"), "sessionID")?;
            run(
                Command::new(client).args(["export", "--pure", &id]),
                &dir,
                "export",
            )?;
            (dir.join("export.stdout"), None)
        }
        Agent::ClaudeCode => (
            find_session(&home.join(".claude/projects"), &grok_id, false)?,
            None,
        ),
        Agent::WorkBuddy => (
            find_session(&home.join(".workbuddy/projects"), &grok_id, false)?,
            None,
        ),
        Agent::KimiCli => {
            let cwd = dir.canonicalize()?;
            let mut matches = Vec::new();
            for state in native_files(&home.join(".kimi-code/sessions"), "state.json")? {
                let value: Value = serde_json::from_slice(&fs::read(&state)?)?;
                if value
                    .get("workDir")
                    .and_then(Value::as_str)
                    .is_some_and(|p| Path::new(p).canonicalize().ok().as_ref() == Some(&cwd))
                {
                    matches.push(state.parent().unwrap().join("agents/main/wire.jsonl"));
                }
            }
            (exactly_one(matches)?, None)
        }
        Agent::ClineCli => {
            let paths = native_files(&dir.join("cline-data"), ".messages.json")?;
            (exactly_one(paths)?, None)
        }
        Agent::CursorCli => {
            let id = cursor_id.unwrap();
            let mut matches = Vec::new();
            for path in native_files(&home.join(".cursor/chats"), "store.db")? {
                let db = rusqlite::Connection::open_with_flags(
                    &path,
                    rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
                )?;
                let encoded: String =
                    db.query_row("SELECT value FROM meta WHERE key='0'", [], |r| r.get(0))?;
                let bytes: Vec<u8> = encoded
                    .as_bytes()
                    .chunks_exact(2)
                    .map(|p| u8::from_str_radix(std::str::from_utf8(p)?, 16).map_err(Into::into))
                    .collect::<Result<_, Box<dyn Error>>>()?;
                let meta: Value = serde_json::from_slice(&bytes)?;
                if meta["agentId"] == id {
                    matches.push(path);
                }
            }
            (exactly_one(matches)?, Some(id))
        }
        Agent::Hermes => {
            let path = home.join(".hermes/state.db");
            let db = rusqlite::Connection::open_with_flags(
                &path,
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
            )?;
            let mut query = db.prepare("SELECT DISTINCT session_id FROM messages WHERE role='user' AND instr(content,?1)>0")?;
            let ids = query
                .query_map([&fixture_name], |r| r.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            if ids.len() != 1 {
                return Err(
                    "expected exactly one Hermes session containing the new task filename".into(),
                );
            }
            (path, Some(ids[0].clone()))
        }
        _ => unreachable!(),
    };
    fs::write(
        dir.join("native-path.txt"),
        native.to_string_lossy().as_bytes(),
    )?;
    if let Some(id) = &database_id {
        fs::write(dir.join("native-id.txt"), id)?;
    }
    let imported = if let Some(id) = database_id {
        import_database(agent, &native, &id, &ReadOptions::default())?
    } else {
        import_session(agent, &native, &ReadOptions::default())?
    };
    let require_usage = !matches!(agent, Agent::CursorCli | Agent::Hermes | Agent::WorkBuddy);
    verify(&imported, &prompt, &fixture_name, &expected, require_usage)
        .map_err(|reason| format!("{client}: {reason}; see private evidence directory"))?;
    // Exercise the same oracle with a wrong answer, not a separate always-failing assertion.
    if verify(
        &imported,
        &prompt,
        &fixture_name,
        "deliberately incorrect answer",
        require_usage,
    )
    .is_ok()
    {
        return Err("acceptance oracle accepted the wrong expected answer".into());
    }
    fs::write(
        dir.join("checks.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "agent":client, "native_import":true, "file_contents":true,
            "user_prompt":true, "assistant_reply":true, "tool_arguments":true,
            "tool_output_and_call_id":true, "event_order":true,
            "usage_checked":require_usage, "usage_present":imported.events.iter().any(|e| matches!(e.value, Event::Usage(_))),
            "source_provenance":true, "wrong_expectation_rejected":true
        }))?,
    )?;
    eprintln!("{client}: native end-to-end checks passed; wrong expectation rejected");
    Ok(())
}

fn verify(
    imported: &SessionImport,
    prompt: &str,
    filename: &str,
    expected: &str,
    require_usage: bool,
) -> Result<(), &'static str> {
    if !imported.summary.is_supported() {
        return Err("native log incomplete or contains unsupported records");
    }
    if imported.events.iter().any(|e| e.sources.is_empty()) {
        return Err("event missing native provenance");
    }
    let user = imported.events.iter().position(
        |e| matches!(&e.value, Event::Message(m) if m.role == Role::User && m.text.contains(prompt)),
    ).ok_or("original user prompt missing")?;
    let (call_index, call) = imported
        .events
        .iter()
        .enumerate()
        .find_map(|(i, e)| {
            let Event::ToolCall(call) = &e.value else {
                return None;
            };
            let args = match &call.arguments {
                ToolArgs::Json(v) => v.to_string(),
                ToolArgs::RawString(v) => v.clone(),
                ToolArgs::Missing => return None,
                _ => return None,
            };
            (i > user && args.contains(filename) && call.id.is_some()).then_some((i, call))
        })
        .ok_or("file-reading tool arguments or call id missing")?;
    let output_index = imported
        .events
        .iter()
        .enumerate()
        .find_map(|(i, e)| {
            let Event::ToolResult(output) = &e.value else {
                return None;
            };
            (i > call_index && output.call_id == call.id && output.text.contains(expected))
                .then_some(i)
        })
        .ok_or("expected tool output not linked to its preceding call")?;
    let last_output = imported
        .events
        .iter()
        .rposition(|e| matches!(e.value, Event::ToolResult(_)))
        .ok_or("tool results missing")?;
    let reply: String = imported.events[last_output + 1..]
        .iter()
        .filter_map(|e| match &e.value {
            Event::Message(m) if m.role == Role::Assistant => Some(m.text.as_str()),
            _ => None,
        })
        .collect();
    if output_index > last_output || reply.trim() != expected.trim() {
        return Err("final assistant reply differs from independent expectation");
    }
    if require_usage
        && !imported.events.iter().any(|e| {
            matches!(&e.value, Event::Usage(u)
        if u.counts.input.is_some_and(|n| n > 0) && u.counts.output.is_some_and(|n| n > 0))
        })
    {
        return Err("native nonzero input/output usage missing");
    }
    Ok(())
}

fn stdout_id(path: &Path, key: &str) -> Result<String, Box<dyn Error>> {
    for line in BufReader::new(File::open(path)?).lines() {
        let line = line?;
        if let Ok(value) = serde_json::from_str::<Value>(&line)
            && let Some(id) = value.get(key).and_then(Value::as_str)
        {
            // IDs are used only to select persisted native records, not as expectations.
            if !id.is_empty()
                && id
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            {
                return Ok(id.to_owned());
            }
        }
    }
    Err("client stdout did not identify a native session".into())
}

fn find_session(root: &Path, id: &str, grok: bool) -> Result<PathBuf, Box<dyn Error>> {
    let mut pending = vec![root.to_path_buf()];
    let mut found = Vec::new();
    while let Some(dir) = pending.pop() {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let ty = entry.file_type()?;
            let path = entry.path();
            if ty.is_dir() {
                pending.push(path);
            } else if ty.is_file() {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                let selected = if grok {
                    name == "updates.jsonl"
                        && path
                            .parent()
                            .and_then(Path::file_name)
                            .is_some_and(|n| n == id)
                } else {
                    name == format!("{id}.jsonl") || name.ends_with(&format!("-{id}.jsonl"))
                };
                if selected {
                    found.push(path);
                }
            }
        }
    }
    if found.len() != 1 {
        return Err("expected exactly one persisted native session for new session id".into());
    }
    Ok(found.remove(0))
}

fn exactly_one(mut paths: Vec<PathBuf>) -> Result<PathBuf, Box<dyn Error>> {
    if paths.len() != 1 {
        return Err("expected exactly one native persistence file for the new session".into());
    }
    Ok(paths.remove(0))
}

fn native_files(root: &Path, suffix: &str) -> io::Result<Vec<PathBuf>> {
    let mut pending = vec![root.to_path_buf()];
    let mut paths = Vec::new();
    while let Some(dir) = pending.pop() {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let ty = entry.file_type()?;
            if ty.is_dir() {
                pending.push(entry.path());
            } else if ty.is_file() && entry.file_name().to_string_lossy().ends_with(suffix) {
                paths.push(entry.path());
            }
        }
    }
    Ok(paths)
}

fn private_file(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

fn run(command: &mut Command, dir: &Path, label: &str) -> TestResult {
    serde_json::to_writer_pretty(
        private_file(&dir.join(format!("{label}.command.json")))?,
        &serde_json::json!({"program":command.get_program().to_string_lossy(),
            "args":command.get_args().map(|a| a.to_string_lossy()).collect::<Vec<_>>() }),
    )?;
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    // File redirection avoids deadlock on verbose clients and avoids printing private transcripts.
    let mut child = command
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(private_file(&dir.join(format!("{label}.stdout")))?)
        .stderr(private_file(&dir.join(format!("{label}.stderr")))?)
        .spawn()?;
    let deadline = Instant::now() + Duration::from_secs(180);
    loop {
        if let Some(status) = child.try_wait()? {
            fs::write(
                dir.join(format!("{label}.exit.json")),
                serde_json::to_vec(
                    &serde_json::json!({"success":status.success(),"code":status.code()}),
                )?,
            )?;
            return if status.success() {
                Ok(())
            } else {
                Err(format!(
                    "{label} failed ({status}); inspect private stderr; not a passing acceptance"
                )
                .into())
            };
        }
        if Instant::now() >= deadline {
            #[cfg(unix)]
            {
                // Stop only this invocation's process group, including shell/tool children.
                let status = Command::new("/bin/kill")
                    .args(["-KILL", &format!("-{}", child.id())])
                    .status()?;
                if !status.success() {
                    child.kill()?;
                }
            }
            #[cfg(not(unix))]
            child.kill()?;
            child.wait()?;
            return Err(
                format!("{label} timed out after 180 seconds; not a passing acceptance").into(),
            );
        }
        thread::sleep(Duration::from_millis(100));
    }
}
