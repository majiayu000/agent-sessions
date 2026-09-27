# Changelog

## 0.2.2 (unreleased)

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
