//! WorkBuddy native project history (CodeBuddy SDK item records).
use super::*;

pub(super) fn workbuddy(v: &Value, state: &mut State, p: &mut Parsed) -> Result<(), LineErrorKind> {
    if let Some(id) = string(v, "sessionId") {
        state.session_id = Some(id);
    }
    if let Some(model) = v.pointer("/providerData/model").and_then(Value::as_str) {
        state.model = Some(model.into());
    }
    p.record_id = string(v, "id");
    p.message_id = p.record_id.clone();
    p.emit(
        0,
        Event::Meta(MetaUpdate {
            session_id: state.session_id.clone(),
            model: state.model.clone(),
            cwd: string(v, "cwd"),
            ..Default::default()
        }),
    );
    match required(v, "type")? {
        "message" => {
            if p.wants(EventKinds::MESSAGE) || p.wants(EventKinds::CONTENT) {
                let role = Role::parse(required(v, "role")?)
                    .ok_or_else(|| LineErrorKind::InvalidField("role".into()))?;
                message(
                    role,
                    v.get("content")
                        .ok_or(LineErrorKind::MissingField("content"))?,
                    v,
                    p,
                )?;
            }
        }
        "function_call" if p.wants(EventKinds::TOOL_CALL) => call(
            string(v, "callId"),
            required(v, "name")?,
            v.get("arguments"),
            3,
            p,
        ),
        "function_call_result" if p.wants(EventKinds::TOOL_RESULT) => {
            let error = match v.get("status").and_then(Value::as_str) {
                Some("completed") => Some(false),
                Some("failed" | "error") => Some(true),
                // Interrupted before the tool recorded an outcome.
                Some("incomplete") => None,
                _ => {
                    p.unknown.push("workbuddy:result-status".into());
                    None
                }
            };
            result(
                string(v, "callId"),
                v.get("output")
                    .ok_or(LineErrorKind::MissingField("output"))?,
                error,
                3,
                p,
            )?;
        }
        // session-meta: CodeBuddy SDK host/runtime/title metadata, no turn text.
        kind @ ("reasoning" | "file-history-snapshot" | "session-meta") => {
            let role = (kind == "reasoning").then_some(Role::Assistant);
            native_content(role, kind, v, 1, p)
        }
        "function_call" | "function_call_result" | "ai-title" => p
            .ignored
            .push(format!("workbuddy:{}", required(v, "type")?)),
        other => p.unknown.push(format!("workbuddy:{other}")),
    }
    Ok(())
}
