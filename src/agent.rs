use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Agent {
    ClaudeCode,
    Codex,
    GeminiCli,
    QwenCode,
    KimiCli,
    Pi,
    CopilotCli,
    CodeBuddy,
    IFlow,
    OpenCode,
    Cline,
    RooCode,
    Goose,
    Continue,
    Cursor,
    Grok,
    ClineCli,
    Hermes,
    WorkBuddy,
    Qoder,
    ZCode,
    GrokBot,
    CursorCli,
    Zed,
    Warp,
    Antigravity,
}

impl Agent {
    /// Sources implemented by this version. See docs/support.md for format scope.
    pub const SUPPORTED: &'static [Self] = &[
        Self::ClaudeCode,
        Self::Codex,
        Self::GeminiCli,
        Self::QwenCode,
        Self::KimiCli,
        Self::Pi,
        Self::CopilotCli,
        Self::CodeBuddy,
        Self::IFlow,
        Self::OpenCode,
        Self::Cline,
        Self::RooCode,
        Self::Goose,
        Self::Continue,
        Self::Cursor,
        Self::Grok,
        Self::ClineCli,
        Self::Hermes,
        Self::WorkBuddy,
        Self::Qoder,
        Self::ZCode,
        Self::GrokBot,
        Self::CursorCli,
        Self::Zed,
        Self::Warp,
        Self::Antigravity,
    ];

    pub(crate) fn streaming(self) -> bool {
        matches!(
            self,
            Self::ClaudeCode
                | Self::Codex
                | Self::QwenCode
                | Self::KimiCli
                | Self::Pi
                | Self::CopilotCli
                | Self::CodeBuddy
                | Self::IFlow
                | Self::WorkBuddy
                | Self::Qoder
        )
    }
}

/// Select one Codex content representation when both are present.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CodexContentMode {
    #[default]
    /// Use session_meta.history_mode; absent metadata means legacy history.
    Auto,
    ResponseItems,
    CompletedItems,
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
