# Changelog

## 0.3.0 (2026-10-08)

- Expand from Claude Code and Codex to 26 coding-agent sources, including native
  JSON/JSONL, patch journals, SQLite and protobuf-backed sessions. See
  [the support matrix](docs/support.md) for each format's scope and verification.
- Add snapshot import, read-only database import/session enumeration, and explicit
  native-directory discovery. Imported events retain physical records, JSON
  pointers or database row identities; unknown structures remain visible.
- Support Codex legacy and paginated content using native history metadata,
  selecting one content source to avoid duplicate messages.
- Preserve native structured tool results and distinguish additional token
  semantics without inventing missing usage or prices.
- Add opt-in real-client end-to-end tests for Codex, Grok Build and OpenCode,
  plus a local Antigravity trajectory exporter and aggregate inspection examples.

The MSRV remains Rust 1.88. Public options/events have additional fields; consumers
constructing struct literals may need to update them when upgrading from 0.2.
Support is limited to the documented formats: this release does not certify every
agent, version, tool type, branch-recovery path or multimodal capability.

## 0.2.2 (2026-09-27)

- Align the Codex statistics fast path with the canonical parser, including error
  kinds, ignored-record validation, and cumulative usage recovery.
- Preserve model context from valid observations even when duplicate usage is
  suppressed.
- Apply `max_file_bytes` to the captured `stop_at_byte` prefix consistently for
  file and stream readers, allowing a larger unread suffix.
- Bound title-index reads to captured file lengths. Existing title APIs remain
  strict and retain their signatures, but now default to a 200 MiB per-index
  limit and an 8 MiB Codex JSONL line limit. Oversized inputs now return an error.
- Add `load_session_titles_with_options` and `TitleReadOptions` to customize or
  disable those limits. Opt-in `TitleErrorMode::Collect` returns valid titles
  together with source/line errors; a nonempty error list means partial results.

The MSRV remains Rust 1.88. No existing public API was removed or renamed.
