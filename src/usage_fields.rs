use crate::{Agent, LineErrorKind};
use serde_json::Value;

pub(crate) fn validate(agent: Agent, v: &Value) -> Result<(), LineErrorKind> {
    let fields: &[&str] = match agent {
        Agent::ClaudeCode => &[
            "/message/id",
            "/message/model",
            "/message/stop_reason",
            "/message/usage/inference_geo",
        ],
        Agent::Codex => &[
            "/payload/id",
            "/payload/cwd",
            "/payload/model",
            "/payload/thread_source",
            "/payload/info/model",
            "/payload/info/model_name",
            "/payload/info/metadata/model",
        ],
    };
    for path in fields {
        if v.pointer(path)
            .is_some_and(|v| !v.is_null() && !v.is_string())
        {
            return Err(LineErrorKind::InvalidField((*path).into()));
        }
    }
    let objects: &[&str] = match agent {
        Agent::ClaudeCode => &["/message"],
        Agent::Codex => &["/payload/info/metadata"],
    };
    for path in objects {
        if v.pointer(path)
            .is_some_and(|v| !v.is_null() && !v.is_object())
        {
            return Err(LineErrorKind::InvalidField((*path).into()));
        }
    }
    Ok(())
}
