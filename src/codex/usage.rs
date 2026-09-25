use crate::{Endpoint, LineErrorKind, TokenSemantics, Usage, UsageBasis};
use crate::{
    parser::{State, string},
    tokens::parse_counts,
};
use serde_json::Value;

pub(super) fn token_count(p: &Value, state: &mut State) -> Result<Option<Usage>, LineErrorKind> {
    // Native token_count with info:null reports rate limits, not usage.
    let info = match p.get("info") {
        Some(Value::Null) => return Ok(None),
        Some(i) if i.is_object() => i,
        _ => return Err(LineErrorKind::MissingField("info")),
    };
    let total = info
        .get("total_token_usage")
        .ok_or(LineErrorKind::MissingField("total_token_usage"))?;
    let total = parse_counts(total, false)?;
    let last = info
        .get("last_token_usage")
        .filter(|v| !v.is_null())
        .map(|v| parse_counts(v, false))
        .transpose()?;
    if !state.broken_usage && state.previous == Some(total) {
        return Ok(None);
    }
    let previous = state.previous.replace(total);
    let broken = std::mem::replace(&mut state.broken_usage, false);
    let (counts, basis) = if let Some(last) = last {
        (last, UsageBasis::LastSample)
    } else if broken {
        return Err(LineErrorKind::LostUsageBaseline);
    } else if let Some(prev) = previous {
        if total.regressed_from(&prev) {
            return Err(LineErrorKind::CounterRegression);
        }
        (total.delta(prev), UsageBasis::CumulativeDelta)
    } else {
        (total, UsageBasis::InitialCumulative)
    };
    if counts.is_missing() {
        return Err(LineErrorKind::LostUsageBaseline);
    }
    Ok(Some(Usage {
        dedup_key: None,
        model: state.model.clone(),
        counts,
        cumulative: Some(total),
        semantics: TokenSemantics::CodexInclusive,
        basis,
        stop_reason: None,
        endpoint: Endpoint::Unknown,
        inference_geo: None,
    }))
}

pub(super) fn response(p: &Value, state: &State) -> Result<Usage, LineErrorKind> {
    let counts = parse_counts(
        p.get("usage").ok_or(LineErrorKind::MissingField("usage"))?,
        false,
    )?;
    Ok(Usage {
        dedup_key: string(p, "response_id").map(|id| format!("codex:{id}")),
        model: string(p, "model").or_else(|| state.model.clone()),
        counts,
        cumulative: None,
        semantics: TokenSemantics::CodexInclusive,
        basis: UsageBasis::Response,
        stop_reason: None,
        endpoint: Endpoint::Unknown,
        inference_geo: None,
    })
}
