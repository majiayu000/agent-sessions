//! Cursor IDE composer bubbles from `state.vscdb` cursorDiskKV.
use super::*;

pub(super) fn cursor(v: &Value, p: &mut Parsed) -> Result<(), LineErrorKind> {
    p.message_id = string(v, "bubbleId");
    // Bubbles rebuilt from agent conversation state store `createdAt: ""`.
    let created = v.get("createdAt").filter(|t| t.as_str() != Some(""));
    p.at = if created.is_some_and(Value::is_string) {
        crate::parser::timestamp(v)?
    } else {
        millis(created)?
    };
    p.timestamp_text = created.and_then(Value::as_str).map(str::to_owned);
    let role = match v.get("type").and_then(Value::as_u64) {
        Some(1) => Role::User,
        Some(2) => Role::Assistant,
        _ => {
            p.unknown.push("cursor:bubble-type".into());
            return Ok(());
        }
    };
    if let Some(text) = v.get("text").filter(|v| !v.is_null()) {
        message(role, text, v, p)?;
    } else {
        p.unknown.push("cursor:missing-or-encrypted-text".into());
    }
    for (slot, key) in ["thinking", "images", "attachedFiles", "context", "richText"]
        .iter()
        .enumerate()
    {
        if let Some(data) = v.get(key).filter(|v| !v.is_null()) {
            native_content(Some(role), key, data, 5 + slot, p);
        }
    }
    // tokenCount is UI/context bookkeeping, not established per-response usage.
    if let Some(tool) = v.get("toolFormerData").filter(|v| !v.is_null()) {
        if let Some(name) = tool.get("name").and_then(Value::as_str) {
            call(
                string(tool, "toolCallId"),
                name,
                tool.get("rawArgs")
                    .filter(|v| !v.is_null())
                    .or_else(|| tool.get("params")),
                3,
                p,
            );
            if let Some(output) = tool.get("result").filter(|v| !v.is_null()) {
                result(
                    string(tool, "toolCallId"),
                    output,
                    tool.get("status")
                        .and_then(Value::as_str)
                        .map(|s| s == "error"),
                    4,
                    p,
                )?;
            }
        } else {
            p.unknown.push("cursor:tool-payload".into());
        }
    } else if v
        .get("toolResults")
        .and_then(Value::as_array)
        .is_some_and(|v| !v.is_empty())
    {
        p.unknown.push("cursor:tool-payload".into());
    }
    Ok(())
}
