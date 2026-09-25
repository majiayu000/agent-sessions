use crate::parser::{Parsed, required, string, text};
use crate::{Event, EventKinds, LineErrorKind, ToolArgs, ToolCall, ToolResult};
use serde_json::Value;

pub(super) fn parse(content: &Value, p: &mut Parsed) -> Result<(), LineErrorKind> {
    if content.is_string() {
        return Ok(());
    }
    let blocks = content
        .as_array()
        .ok_or(LineErrorKind::InvalidField("content".into()))?;
    for (index, b) in blocks.iter().enumerate() {
        match required(b, "type")? {
            "tool_use" | "server_tool_use" if p.wants(EventKinds::TOOL_CALL) => p.emit(
                3 + index,
                Event::ToolCall(ToolCall {
                    id: string(b, "id"),
                    name: required(b, "name")?.to_owned(),
                    arguments: b
                        .get("input")
                        .map_or(ToolArgs::Missing, |v| ToolArgs::Json(v.clone())),
                }),
            ),
            "tool_result" if p.wants(EventKinds::TOOL_RESULT) => {
                let content = b
                    .get("content")
                    .ok_or(LineErrorKind::MissingField("tool_result.content"))?;
                let body = text(content, p)?;
                p.emit(
                    3 + index,
                    Event::ToolResult(ToolResult {
                        call_id: string(b, "tool_use_id"),
                        is_error: b.get("is_error").and_then(Value::as_bool),
                        text: body,
                    }),
                );
            }
            _ => {}
        }
    }
    Ok(())
}
