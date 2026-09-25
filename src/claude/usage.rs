use crate::parser::string;
use crate::{Endpoint, LineErrorKind, TokenSemantics, Usage, UsageBasis};
use serde_json::Value;

pub(super) fn parse(
    message: &Value,
    u: &Value,
    policy: crate::AccountingPolicy,
) -> Result<Usage, LineErrorKind> {
    let inference_geo = string(u, "inference_geo");
    let endpoint = match inference_geo.as_deref() {
        Some("not_available") => Endpoint::Native,
        Some("") => Endpoint::Proxy,
        _ => Endpoint::Unknown,
    };
    let (counts, adjustments) = crate::accounting::counts(u, true, policy)?;
    Ok(Usage {
        adjustments,
        dedup_key: string(message, "id").map(|id| format!("claude:{id}")),
        model: string(message, "model"),
        counts,
        cumulative: None,
        semantics: TokenSemantics::ClaudeExclusive,
        basis: UsageBasis::Message,
        stop_reason: string(message, "stop_reason"),
        endpoint,
        inference_geo,
    })
}
