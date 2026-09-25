use crate::parser::{Parsed, required, string, text};
use crate::{Event, LineErrorKind, ToolArgs, ToolCall, ToolResult};
use serde_json::Value;

pub(super) fn parse(kind: &str, v: &Value, p: &mut Parsed) -> Result<(), LineErrorKind> {
    match kind {
        "function_call" | "custom_tool_call" => {
            let key = if kind == "function_call" {
                "arguments"
            } else {
                "input"
            };
            let args = match v.get(key) {
                Some(Value::String(s)) => ToolArgs::RawString(s.clone()),
                Some(v) => ToolArgs::Json(v.clone()),
                None => ToolArgs::Missing,
            };
            p.emit(
                3,
                Event::ToolCall(ToolCall {
                    kind: if kind == "custom_tool_call" {
                        crate::ToolCallKind::Custom
                    } else {
                        crate::ToolCallKind::Function
                    },
                    id: string(v, "call_id"),
                    name: required(v, "name")?.to_owned(),
                    arguments: args,
                }),
            );
        }
        "function_call_output" | "custom_tool_call_output" => {
            let output = v
                .get("output")
                .ok_or(LineErrorKind::MissingField("output"))?;
            let body = text(output, p)?;
            p.emit(
                3,
                Event::ToolResult(ToolResult {
                    call_id: string(v, "call_id"),
                    is_error: v.get("is_error").and_then(Value::as_bool),
                    text: body,
                }),
            );
        }
        _ => {}
    }
    Ok(())
}
