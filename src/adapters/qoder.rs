//! Qoder project/task JSONL: Claude-style messages plus Qoder SDK metadata entries.
use super::*;

pub(super) fn qoder(v: &Value, state: &mut State, p: &mut Parsed) -> Result<(), LineErrorKind> {
    let kind = v
        .get("type")
        .or_else(|| v.get("role"))
        .and_then(Value::as_str)
        .ok_or(LineErrorKind::MissingField("type or role"))?;
    if let Some(id) = string(v, "sessionId") {
        state.session_id = Some(id);
    }
    match kind {
        "user" | "assistant" => {
            let mut native = v.clone();
            native["type"] = Value::String(kind.into());
            if native.pointer("/message/role").is_none() {
                native
                    .get_mut("message")
                    .and_then(Value::as_object_mut)
                    .ok_or_else(|| LineErrorKind::InvalidField("message".into()))?
                    .insert("role".into(), Value::String(kind.into()));
            }
            anthropic_session(&native, state, p)
        }
        "runtime-config" => {
            state.model = string(v, "model");
            p.emit(
                0,
                Event::Meta(MetaUpdate {
                    session_id: state.session_id.clone(),
                    model: state.model.clone(),
                    ..Default::default()
                }),
            );
            Ok(())
        }
        "session_meta" => {
            match v.pointer("/data/meta_type").and_then(Value::as_str) {
                Some("session_info" | "slash_command") => {
                    p.ignored.push("qoder:session-metadata".into())
                }
                _ => p.unknown.push("qoder:session-metadata-variant".into()),
            }
            Ok(())
        }
        // SDK leaf pointer: automatic entries follow the newest appended
        // record. A user-selected (rewound) leaf would hide appended entries.
        "active-leaf" => {
            if v.get("explicit").and_then(Value::as_bool) == Some(false)
                && v.get("rewound").is_none_or(|r| r == false)
            {
                p.ignored.push("qoder:active-leaf".into())
            } else {
                p.unknown.push("qoder:active-leaf-explicit".into())
            }
            Ok(())
        }
        "workspace-directories" => {
            p.ignored.push("qoder:workspace-directories".into());
            Ok(())
        }
        _ => crate::claude::parse(v, state, p),
    }
}
