//! Bounded Claude Code and Codex JSONL readers.
//!
//! Events preserve source semantics and provenance. Check [`ReadSummary`] before
//! committing a snapshot; receiving events alone does not establish completeness.
#![doc = include_str!("../README.md")]
mod agent;
mod claude;
mod codex;
mod discover;
mod error;
mod event;
mod meta;
mod parser;
mod reader;
mod roots;
mod tokens;

pub use agent::*;
pub use discover::*;
pub use error::*;
pub use event::*;
pub use meta::*;
pub use reader::*;
pub use roots::*;
pub use tokens::*;

/// Package version, available for consumer cache keys.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
