use crate::{
    AccountingPolicy, Endpoint, LineErrorKind, TokenSemantics, Usage, UsageAdjustment, UsageBasis,
};
use crate::{
    accounting::counts,
    parser::{State, string},
};
use serde_json::Value;

pub(super) fn token_count(
    p: &Value,
    state: &mut State,
    policy: AccountingPolicy,
) -> Result<Option<Usage>, LineErrorKind> {
    let statistics = policy == AccountingPolicy::UsageStatistics;
    let info = match p.get("info") {
        Some(Value::Null) => return Ok(None),
        None if statistics || p.get("rate_limits").is_some() => return Ok(None),
        Some(i) if i.is_object() => i,
        _ => return Err(LineErrorKind::MissingField("info")),
    };
    let total = match info.get("total_token_usage").filter(|v| !v.is_null()) {
        Some(total) => total,
        None if statistics => return Ok(None),
        None => return Err(LineErrorKind::MissingField("total_token_usage")),
    };
    let (total, mut adjustments) = counts(total, false, policy)?;
    let last = info
        .get("last_token_usage")
        .filter(|v| !v.is_null())
        .map(|v| counts(v, false, policy))
        .transpose()?;
    let duplicate = state.previous.is_some_and(|prev| {
        if statistics {
            total.statistics_eq(prev)
        } else {
            total == prev
        }
    });
    if (!state.broken_usage || statistics) && duplicate {
        return Ok(None);
    }
    let previous = state.previous.replace(total);
    let broken = std::mem::replace(&mut state.broken_usage, false);
    let (counts, basis) = if let Some((last, changes)) = last {
        adjustments.extend(changes);
        (last, UsageBasis::LastSample)
    } else {
        if broken {
            if !statistics {
                return Err(LineErrorKind::LostUsageBaseline);
            }
            adjustments.push(UsageAdjustment::UncertainCumulativeBaseline);
        }
        if let Some(prev) = previous {
            if total.regressed_from(&prev) {
                if !statistics {
                    return Err(LineErrorKind::CounterRegression);
                }
                adjustments.push(UsageAdjustment::CumulativeRegression);
            }
            if statistics {
                if total
                    .values()
                    .into_iter()
                    .zip(prev.values())
                    .any(|(a, b)| a.is_some() && b.is_none())
                {
                    adjustments.push(UsageAdjustment::MissingCumulativeBaseline);
                }
                (total.statistics_delta(prev), UsageBasis::CumulativeDelta)
            } else {
                (total.delta(prev), UsageBasis::CumulativeDelta)
            }
        } else {
            (total, UsageBasis::InitialCumulative)
        }
    };
    if counts.is_missing() && !statistics {
        return Err(LineErrorKind::LostUsageBaseline);
    }
    Ok(Some(Usage {
        adjustments,
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
    let (counts, adjustments) = counts(
        p.get("usage").ok_or(LineErrorKind::MissingField("usage"))?,
        false,
        AccountingPolicy::Strict,
    )?;
    Ok(Usage {
        adjustments,
        dedup_key: string(p, "response_id").map(|id| format!("codex:{id}")),
        model: string(p, "model")
            .filter(|s| !s.trim().is_empty())
            .or_else(|| state.model.clone()),
        counts,
        cumulative: None,
        semantics: TokenSemantics::CodexInclusive,
        basis: UsageBasis::Response,
        stop_reason: None,
        endpoint: Endpoint::Unknown,
        inference_geo: None,
    })
}
