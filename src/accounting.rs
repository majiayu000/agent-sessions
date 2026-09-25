use crate::{AccountingPolicy, LineErrorKind, TokenCounts};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum UsageAdjustment {
    NegativeClamped { field: String, original: i64 },
    CacheTtlCapped { original: u64, total: u64 },
    UnreportedCounters,
    MissingCumulativeBaseline,
    CumulativeRegression,
    UncertainCumulativeBaseline,
}

pub(crate) fn counts(
    v: &Value,
    claude: bool,
    policy: AccountingPolicy,
) -> Result<(TokenCounts, Vec<UsageAdjustment>), LineErrorKind> {
    let statistics = policy == AccountingPolicy::UsageStatistics;
    let mut corrected: Option<Value> = None;
    let mut adjustments = Vec::new();
    if statistics && claude {
        for key in [
            "input_tokens",
            "output_tokens",
            "cache_creation_input_tokens",
            "cache_read_input_tokens",
        ] {
            if let Some(original) = v.get(key).and_then(Value::as_i64).filter(|n| *n < 0) {
                corrected.get_or_insert_with(|| v.clone())[key] = Value::from(0);
                adjustments.push(UsageAdjustment::NegativeClamped {
                    field: key.into(),
                    original,
                });
            }
        }
        if let Some(original) = v
            .pointer("/cache_creation/ephemeral_1h_input_tokens")
            .and_then(Value::as_i64)
            .filter(|n| *n < 0)
        {
            corrected.get_or_insert_with(|| v.clone())["cache_creation"]["ephemeral_1h_input_tokens"] =
                Value::from(0);
            adjustments.push(UsageAdjustment::NegativeClamped {
                field: "ephemeral_1h_input_tokens".into(),
                original,
            });
        }
        let current = corrected.as_ref().unwrap_or(v);
        let total = current
            .get("cache_creation_input_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        if let Some(original) = current
            .pointer("/cache_creation/ephemeral_1h_input_tokens")
            .and_then(Value::as_u64)
            .filter(|n| *n > total)
        {
            corrected.get_or_insert_with(|| v.clone())["cache_creation"]["ephemeral_1h_input_tokens"] =
                Value::from(total);
            adjustments.push(UsageAdjustment::CacheTtlCapped { original, total });
        }
    }
    let counts = crate::tokens::parse_counts(corrected.as_ref().unwrap_or(v), claude, statistics)?;
    if counts.is_missing() {
        adjustments.push(UsageAdjustment::UnreportedCounters);
    }
    Ok((counts, adjustments))
}
