//! Bounded Claude Code and Codex JSONL readers.
//!
//! Events preserve source semantics and provenance. Check [`ReadSummary`] before
//! committing a snapshot; receiving events alone does not establish completeness.
#![doc = include_str!("../README.md")]
mod accounting;
mod agent;
mod claude;
mod codex;
mod discover;
mod error;
mod event;
mod history;
mod meta;
mod parser;
mod raw;
mod reader;
mod roots;
mod titles;
mod tokens;

pub use accounting::UsageAdjustment;
pub use agent::*;
pub use discover::*;
pub use error::*;
pub use event::*;
pub use history::*;
pub use meta::*;
pub use raw::*;
pub use reader::*;
pub use roots::*;
pub use titles::*;
pub use tokens::*;

/// Package version, available for consumer cache keys.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
