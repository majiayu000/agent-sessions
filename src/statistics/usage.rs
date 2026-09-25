//! Typed token counters avoid allocating JSON maps on the hot usage path.
use super::{
    DecodeError,
    header::{self, Header},
};
use crate::parser::{Parsed, State};
use crate::{Event, LineErrorKind, ReadOptions, TokenCounts, UsageAdjustment};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::value::RawValue;
use std::borrow::Cow;

#[derive(Deserialize)]
struct Info<'a> {
    total_token_usage: Option<Counters>,
    last_token_usage: Option<Counters>,
    #[serde(borrow)]
    model: Option<Cow<'a, str>>,
    #[serde(borrow)]
    model_name: Option<Cow<'a, str>>,
    #[serde(borrow)]
    metadata: Option<Metadata<'a>>,
}
#[derive(Deserialize)]
struct Metadata<'a> {
    #[serde(borrow)]
    model: Option<Cow<'a, str>>,
}
#[derive(Deserialize)]
struct CacheCreation {
    ephemeral_1h_input_tokens: Option<u64>,
}
#[derive(Deserialize)]
struct Counters {
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    cached_input_tokens: Option<u64>,
    cache_read_input_tokens: Option<u64>,
    cache_write_input_tokens: Option<u64>,
    reasoning_output_tokens: Option<u64>,
    total_tokens: Option<u64>,
    cache_creation: Option<CacheCreation>,
}
impl Counters {
    fn has_nonzero(&self) -> bool {
        [
            self.input_tokens,
            self.output_tokens,
            self.cached_input_tokens,
            self.cache_read_input_tokens,
            self.cache_write_input_tokens,
            self.reasoning_output_tokens,
            self.total_tokens,
            self.cache_creation
                .as_ref()
                .and_then(|c| c.ephemeral_1h_input_tokens),
        ]
        .into_iter()
        .flatten()
        .any(|n| n > 0)
    }
    fn counts(self) -> Result<(TokenCounts, Vec<UsageAdjustment>), DecodeError> {
        let counts = TokenCounts {
            input: self.input_tokens,
            output: self.output_tokens,
            cache_read: self.cached_input_tokens.or(self.cache_read_input_tokens),
            cache_write: self.cache_write_input_tokens,
            cache_write_1h: self
                .cache_creation
                .and_then(|c| c.ephemeral_1h_input_tokens),
            reasoning: self.reasoning_output_tokens,
            reported_total: self.total_tokens,
        };
        if matches!((counts.cache_write_1h,counts.cache_write),(Some(h),Some(w)) if h>w) {
            return Err(DecodeError::Fields(LineErrorKind::InvalidField(
                "cache_creation".into(),
            )));
        }
        let changes = if counts.is_missing() {
            vec![UsageAdjustment::UnreportedCounters]
        } else {
            Vec::new()
        };
        Ok((counts, changes))
    }
}
fn text<'a>(value: Option<&'a RawValue>, field: &str) -> Result<Option<Cow<'a, str>>, DecodeError> {
    match value {
        None => Ok(None),
        Some(v) if v.get() == "null" => Ok(None),
        v => header::string(v)
            .map(Some)
            .ok_or_else(|| DecodeError::Fields(LineErrorKind::InvalidField(field.into()))),
    }
}
pub(super) fn decode(
    h: &Header<'_>,
    state: &mut State,
    opts: &ReadOptions,
    at: Option<DateTime<Utc>>,
) -> Result<Parsed, DecodeError> {
    let Some(payload) = &h.payload else {
        return Err(DecodeError::Fields(LineErrorKind::MissingField("payload")));
    };
    let Some(raw) = payload.info.filter(|v| v.get() != "null") else {
        return Ok(Parsed::default());
    };
    let info: Info<'_> = serde_json::from_str(raw.get())?;
    let timestamp = header::string(h.timestamp).ok_or(DecodeError::Fields(
        LineErrorKind::MissingField("timestamp"),
    ))?;
    let payload_model = text(payload.model, "/payload/model")?;
    for (value, field) in [
        (payload.id, "/payload/id"),
        (payload.cwd, "/payload/cwd"),
        (payload.thread_source, "/payload/thread_source"),
    ] {
        text(value, field)?;
    }
    let Some(total) = info.total_token_usage else {
        return Ok(Parsed {
            ignored_one: info
                .last_token_usage
                .as_ref()
                .filter(|last| last.has_nonzero())
                .map(|_| (true, Cow::Borrowed(crate::CODEX_MISSING_TOTAL_USAGE))),
            ..Default::default()
        });
    };
    let (total, adjustments) = total.counts()?;
    let last = info.last_token_usage.map(Counters::counts).transpose()?;
    let observed = [
        info.model,
        info.model_name,
        info.metadata.and_then(|m| m.model),
        payload_model,
    ]
    .into_iter()
    .flatten()
    .find(|model| !model.trim().is_empty());
    let mut parsed = Parsed {
        at,
        include: opts.include,
        accounting: opts.accounting,
        timestamp_text: Some(timestamp.into_owned()),
        record_id: header::string(h.uuid).map(Cow::into_owned),
        record_type: Some("event_msg".into()),
        ..Default::default()
    };
    if let Some(mut usage) =
        crate::codex::usage::from_counts(total, last, adjustments, state, opts.accounting)
            .map_err(DecodeError::Fields)?
    {
        crate::codex::usage::apply_model(&mut usage, state, observed.map(Cow::into_owned));
        parsed.emit(2, Event::Usage(usage));
    }
    Ok(parsed)
}
