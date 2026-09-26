//! A selective, borrowed decode for statistics. Irrelevant payload bodies never
//! become Value trees; selected records use the same semantic parser as normal.
mod header;
mod usage;
use crate::parser::{Parsed, State, parse};
use crate::{AccountingPolicy, Agent, CodexUsageMode, EventKinds, LineErrorKind, ReadOptions};
use serde_json::Value;

pub(crate) enum DecodeError {
    Json(serde_json::Error),
    Fields(LineErrorKind),
}
impl From<serde_json::Error> for DecodeError {
    fn from(e: serde_json::Error) -> Self {
        Self::Json(e)
    }
}

pub(crate) fn decode(
    agent: Agent,
    bytes: &[u8],
    state: &mut State,
    opts: &ReadOptions,
) -> Result<Parsed, DecodeError> {
    if agent == Agent::Codex
        && opts.accounting == AccountingPolicy::UsageStatistics
        && !opts.include.contains(EventKinds::MESSAGE)
        && !opts.include.contains(EventKinds::TOOL_CALL)
        && !opts.include.contains(EventKinds::TOOL_RESULT)
        && let Ok(header::Object(header)) =
            serde_json::from_slice::<header::Object<header::Header<'_>>>(bytes)
    {
        if header.kind().as_deref() == Some("event_msg")
            && opts.codex_usage == CodexUsageMode::TokenCount
            && opts.include.contains(EventKinds::USAGE)
            && header.payload.as_ref().and_then(|p| p.kind()).as_deref() == Some("token_count")
        {
            // Only accept a fully validated fast result. Unsupported envelopes
            // and field errors use the canonical decoder, including its error
            // precedence and cumulative-baseline recovery contract.
            let mut candidate = state.clone();
            if let Ok(at) = header.timestamp()
                && let Ok(Some(parsed)) = usage::decode(&header, &mut candidate, opts, at)
            {
                *state = candidate;
                return Ok(parsed);
            }
        }
        // Preserve semantic validation for ignored envelopes too. This small
        // projection still skips unrequested message and tool bodies.
        let value = header.value()?;
        return parse(
            agent,
            &value,
            state,
            opts.codex_usage,
            opts.include,
            opts.accounting,
        )
        .map_err(DecodeError::Fields);
    }
    let value: Value = serde_json::from_slice(bytes)?;
    parse(
        agent,
        &value,
        state,
        opts.codex_usage,
        opts.include,
        opts.accounting,
    )
    .map_err(DecodeError::Fields)
}

#[cfg(test)]
mod tests;
