use crate::LineErrorKind;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenCounts {
    pub input: Option<u64>,
    pub output: Option<u64>,
    pub cache_read: Option<u64>,
    pub cache_write: Option<u64>,
    /// Subset of cache_write, never an additional category to sum.
    pub cache_write_1h: Option<u64>,
    pub reasoning: Option<u64>,
    pub reported_total: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum TokenSemantics {
    ClaudeExclusive,
    CodexInclusive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum UsageBasis {
    Message,
    LastSample,
    CumulativeDelta,
    InitialCumulative,
    Response,
}

impl TokenCounts {
    pub fn is_missing(&self) -> bool {
        self.values().iter().all(Option::is_none)
    }
    pub(crate) fn values(&self) -> [Option<u64>; 7] {
        [
            self.input,
            self.output,
            self.cache_read,
            self.cache_write,
            self.cache_write_1h,
            self.reasoning,
            self.reported_total,
        ]
    }
    pub(crate) fn statistics_eq(self, other: Self) -> bool {
        self.values()
            .into_iter()
            .zip(other.values())
            .all(|(a, b)| a.unwrap_or(0) == b.unwrap_or(0))
    }
    pub(crate) fn statistics_delta(self, prev: Self) -> Self {
        fn sub(a: Option<u64>, b: Option<u64>) -> Option<u64> {
            a.map(|a| a.saturating_sub(b.unwrap_or(0)))
        }
        Self {
            input: sub(self.input, prev.input),
            output: sub(self.output, prev.output),
            cache_read: sub(self.cache_read, prev.cache_read),
            cache_write: sub(self.cache_write, prev.cache_write),
            cache_write_1h: sub(self.cache_write_1h, prev.cache_write_1h),
            reasoning: sub(self.reasoning, prev.reasoning),
            reported_total: sub(self.reported_total, prev.reported_total),
        }
    }
    pub(crate) fn regressed_from(&self, prev: &Self) -> bool {
        self.values()
            .into_iter()
            .zip(prev.values())
            .any(|(a, b)| matches!((a,b), (Some(a),Some(b)) if a < b))
    }
    pub(crate) fn delta(self, prev: Self) -> Self {
        fn sub(a: Option<u64>, b: Option<u64>) -> Option<u64> {
            a?.checked_sub(b?)
        }
        Self {
            input: sub(self.input, prev.input),
            output: sub(self.output, prev.output),
            cache_read: sub(self.cache_read, prev.cache_read),
            cache_write: sub(self.cache_write, prev.cache_write),
            cache_write_1h: sub(self.cache_write_1h, prev.cache_write_1h),
            reasoning: sub(self.reasoning, prev.reasoning),
            reported_total: sub(self.reported_total, prev.reported_total),
        }
    }
    /// Convert inclusive Codex counts into disjoint categories without guessing
    /// absent counters. An unprovable subtraction stays None.
    pub fn exclusive(self, semantics: TokenSemantics) -> Option<Self> {
        if semantics == TokenSemantics::ClaudeExclusive {
            return Some(self);
        }
        let input = match (self.input, self.cache_read, self.cache_write) {
            (Some(i), Some(r), Some(w)) => Some(i.checked_sub(r)?.checked_sub(w)?),
            _ => None,
        };
        let output = match (self.output, self.reasoning) {
            (Some(o), Some(r)) => Some(o.checked_sub(r)?),
            _ => None,
        };
        Some(Self {
            input,
            output,
            ..self
        })
    }
}

fn count(v: &Value, key: &str) -> Result<Option<u64>, LineErrorKind> {
    match v.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(n) => n
            .as_u64()
            .map(Some)
            .ok_or(LineErrorKind::InvalidField(key.into())),
    }
}

pub(crate) fn parse_counts(
    v: &Value,
    claude: bool,
    allow_missing: bool,
) -> Result<TokenCounts, LineErrorKind> {
    if !v.is_object() {
        return Err(LineErrorKind::InvalidField("usage".into()));
    }
    let cache_read = if claude {
        count(v, "cache_read_input_tokens")?
    } else {
        count(v, "cached_input_tokens")?.or(count(v, "cache_read_input_tokens")?)
    };
    let result = TokenCounts {
        input: count(v, "input_tokens")?,
        output: count(v, "output_tokens")?,
        cache_read,
        cache_write: count(
            v,
            if claude {
                "cache_creation_input_tokens"
            } else {
                "cache_write_input_tokens"
            },
        )?,
        cache_write_1h: match v.get("cache_creation") {
            None | Some(Value::Null) => None,
            Some(c) if c.is_object() => count(c, "ephemeral_1h_input_tokens")?,
            _ => return Err(LineErrorKind::InvalidField("cache_creation".into())),
        },
        reasoning: count(v, "reasoning_output_tokens")?,
        reported_total: count(v, "total_tokens")?,
    };
    if result.is_missing() && !allow_missing {
        return Err(LineErrorKind::MissingField("usage counters"));
    }
    if matches!((result.cache_write_1h,result.cache_write),(Some(h),Some(w)) if h>w) {
        return Err(LineErrorKind::InvalidField("cache_creation".into()));
    }
    Ok(result)
}
