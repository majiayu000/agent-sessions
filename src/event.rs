use crate::{Endpoint, Origin, Role, TokenCounts, TokenSemantics, UsageBasis};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Location {
    pub record_index: u64,
    pub line_no: u64,
    pub byte_start: u64,
    pub byte_end: u64,
    pub event_index: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Located<T> {
    pub location: Location,
    pub at: Option<DateTime<Utc>>,
    pub session_id: Option<String>,
    pub message_id: Option<String>,
    pub value: T,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Event {
    Meta(MetaUpdate),
    Message(Message),
    ToolCall(ToolCall),
    ToolResult(ToolResult),
    Usage(Usage),
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MetaUpdate {
    pub session_id: Option<String>,
    pub cwd: Option<String>,
    pub git_branch: Option<String>,
    pub model: Option<String>,
    pub origin: Option<Origin>,
    pub agent_version: Option<String>,
    /// Native provenance fields; classification never replaces this evidence.
    pub source: Option<Value>,
    pub originator: Option<String>,
    pub thread_source: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub text: String,
    pub text_segments: Vec<std::ops::Range<usize>>,
    pub is_meta: bool,
    pub is_sidechain: bool,
    pub parent_id: Option<String>,
}
impl Message {
    /// Original first text block, including an empty first block.
    pub fn first_text(&self) -> Option<&str> {
        self.text.get(self.text_segments.first()?.clone())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: Option<String>,
    pub name: String,
    pub arguments: ToolArgs,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ToolArgs {
    Json(Value),
    RawString(String),
    Missing,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolResult {
    pub call_id: Option<String>,
    pub is_error: Option<bool>,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    /// Source identity only. Scope with host/session when deduplicating.
    /// None requires the consumer to use physical occurrence identity.
    pub dedup_key: Option<String>,
    pub model: Option<String>,
    pub counts: TokenCounts,
    pub cumulative: Option<TokenCounts>,
    pub semantics: TokenSemantics,
    pub basis: UsageBasis,
    pub stop_reason: Option<String>,
    pub endpoint: Endpoint,
    /// Undocumented Claude field; endpoint classification is only a heuristic.
    pub inference_geo: Option<String>,
}

/// Select projections. Relevant metadata continuity is always maintained;
/// excluded payloads are not validated or materialized.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EventKinds(u8);
impl Default for EventKinds {
    fn default() -> Self {
        Self::ALL
    }
}
impl EventKinds {
    pub const META: Self = Self(1);
    pub const MESSAGE: Self = Self(2);
    pub const TOOL_CALL: Self = Self(4);
    pub const TOOL_RESULT: Self = Self(8);
    pub const USAGE: Self = Self(16);
    pub const ALL: Self = Self(31);
    pub fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
    pub fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
    pub(crate) fn includes(self, event: &Event) -> bool {
        let bit = match event {
            Event::Meta(_) => 1,
            Event::Message(_) => 2,
            Event::ToolCall(_) => 4,
            Event::ToolResult(_) => 8,
            Event::Usage(_) => 16,
        };
        self.0 & bit != 0
    }
}
