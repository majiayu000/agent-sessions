use crate::Origin;
use serde_json::Value;

pub(crate) fn classify(p: &Value) -> Origin {
    if p.get("thread_source")
        .and_then(Value::as_str)
        .is_some_and(|s| s.eq_ignore_ascii_case("subagent"))
    {
        return Origin::Subagent;
    }
    if let Some(s) = p.get("source") {
        if s.is_object() {
            for (key, origin) in [
                ("subagent", Origin::Subagent),
                ("exec", Origin::Exec),
                ("vscode", Origin::Ide),
                ("cli", Origin::Interactive),
            ] {
                if s.get(key).is_some() {
                    return origin;
                }
            }
        }
        if let Some(s) = s.as_str() {
            match s.trim().to_ascii_lowercase().as_str() {
                "subagent" => return Origin::Subagent,
                "exec" => return Origin::Exec,
                "vscode" => return Origin::Ide,
                "cli" | "interactive" => return Origin::Interactive,
                _ => {}
            }
        }
    }
    if p.get("thread_source").and_then(Value::as_str) == Some("automation") {
        return Origin::Exec;
    }
    match p.get("originator").and_then(Value::as_str) {
        Some("codex_exec" | "symphony-orchestrator") => Origin::Exec,
        Some("codex-tui" | "codex_cli_rs") => Origin::Interactive,
        Some("Codex Desktop" | "codex_work_desktop") => Origin::Ide,
        _ => Origin::Unknown,
    }
}
