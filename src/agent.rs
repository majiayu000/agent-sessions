use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Agent {
    ClaudeCode,
    Codex,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum FileKind {
    Main,
    Subagent,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Origin {
    #[default]
    Unknown,
    Interactive,
    Ide,
    Exec,
    Subagent,
}

impl Origin {
    pub(crate) fn priority(self) -> u8 {
        match self {
            Self::Unknown => 0,
            Self::Interactive => 1,
            Self::Ide => 2,
            Self::Exec => 3,
            Self::Subagent => 4,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Role {
    User,
    Assistant,
    System,
    Developer,
}

impl Role {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "user" => Some(Self::User),
            "assistant" => Some(Self::Assistant),
            "system" => Some(Self::System),
            "developer" => Some(Self::Developer),
            _ => None,
        }
    }
}

/// The ledger to emit; selection is explicit to prevent double counting.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum CodexUsageMode {
    #[default]
    TokenCount,
    Response,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Endpoint {
    Native,
    Proxy,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AccountingPolicy {
    #[default]
    Strict,
    /// Explicit legacy statistical normalization; adjustments remain observable.
    UsageStatistics,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ToolCallKind {
    Function,
    Custom,
    Server,
}
