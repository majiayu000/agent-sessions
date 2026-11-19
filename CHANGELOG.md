# Changelog

## 0.4.1 (2026-11-19)

Compatible with 0.4.0: no dependency, minimum Rust (1.89) or public API removal.
`Agent` is `#[non_exhaustive]`, so the new `Crush` and `OpenHands` variants do
not break exhaustive matches. Records that 0.4.0 left unknown may now appear as
Content or tool results, or be ignored as metadata.

All entries below come from a read-only sweep of real local histories with the
new aggregate-only `examples/corpus_sweep.rs`; results are in
[docs/support.md](docs/support.md#2026-11-17-本机真实历史全量扫描).

- New source **Crush** (`.crush/crush.db`, `import_database`): text, tool calls,
  tool-result rows, reasoning/media/run reports/finish parts as Content,
  compaction summaries as Content, child sessions as sidechains. Session token
  columns are a last-step snapshot, so no Usage is produced.
- New source **OpenHands CLI** (`~/.openhands/conversations/<id>/`,
  `import_session` on the directory or its `base_state.json`): messages,
  action thoughts, tool calls, observations, agent errors and rejections;
  system prompt, condensation, state and hook events as Content; per-call token
  ledger as Usage. Event times have no offset, so only the native text is kept.
- **Kilo Code** 7.x stores sessions in OpenCode's SQLite schema
  (`~/.local/share/kilo/kilo.db`); read it with `Agent::OpenCode`.
- Claude Code 2.1.30x: request snapshots (`api-request`, `api-request-blob`,
  `api-request-shape`), session links (`relocated`, `fork-context-ref`,
  `continued-in`), `worktree-state` and artifact state are Content, never extra
  messages; server `*_tool_result` blocks are tool results; `fallback` model
  switches are Content; `session-owner`, `tools-used`, `launched`, `failed` and
  `tag` are ignored metadata.
- Codex: paginated `local_image` references are Content; realtime voice session
  start/close markers are ignored, other realtime payloads stay unknown.
- WorkBuddy 5.x: CodeBuddy SDK `session-meta` is Content; an `incomplete` tool
  result (interrupted) keeps an unknown outcome instead of being unknown.
- The Cursor Agent CLI descriptor generator lives in
  `scripts/cursor-agent-descriptor/`.
- Grok Build 1.0.7x: name server-side search calls (`_meta.backend`) from their
  native `rawInput.variant` (WebSearch/XSearch); merge `_meta` across tool
  updates so a failed `toolNotRun` update keeps the original tool name; keep
  auto-compaction, compaction checkpoints, image drop/compress notices, workflow/goal progress
  and scheduled-task records as native Content without emitting their
  progress token counters as Usage; ignore `memory_flush_started`.
- Kimi Code 2.x: keep the agent-loop engine history (`agent.message.appended`)
  as native Content so its mirrored text and `meta.usage` are not counted
  twice; keep text-bearing bookkeeping (system prompt binding, steer input,
  step interruptions, approvals, interactions, background tasks, plugin
  reminders, MCP tool discovery) as Content; record turn/runtime/prompt
  lifecycle, `token_counting.*` context-size estimates and file-history
  checkpoints as ignored. Move per-record Kimi v2 classification into its own
  module.
- Cursor Agent CLI: regenerate the protobuf descriptors from CLI
  2026.10.22-27dcf5f (field 46 `writes_voice_call_utterances` and other new
  fields), keeping definitions removed upstream so older stores still decode;
  read never-checkpointed stores (empty `latestRootBlobId`) as empty sessions;
  keep voice-call utterances as native Content. Cursor IDE bubbles with
  `createdAt: ""` read without a timestamp. Qoder automatic `active-leaf`
  pointers are metadata; user-selected leaves stay unknown.

## 0.4.0 (2026-11-16)

See [upgrade notes](docs/release-0.4.md).

- Raise the minimum Rust version to 1.89 and align
  rusqlite with 0.41.1 for the active ccstats SQLite graph. Published 0.3.0 and
  its Rust 1.88 contract are unchanged.

- Avoid the unused contiguous copy for CodeBuddy/Pi/Kimi/Grok replay imports;
  document-only imports now retain only the contiguous document. Gemini keeps
  both representations for document/JSONL fallback and physical provenance.
- Add an explicit-file native-history comparison example for Claude and Codex,
  checking text, tools, selected native content and byte provenance without
  printing private payloads. Claude 2.1.295 artifact ledger records remain
  unknown; successful field comparison is not a full-support claim.

- Preserve native non-text content through `Event::Content` and
  `EventKinds::CONTENT`, including thinking, signatures, media references,
  attachments and context controls. Keep structured Claude/Codex tool outputs.
- Fold Kimi v2 undo, clear and compaction; assemble old Kimi subagent wire
  fragments. Resolve Pi's current branch, context edits and compaction while
  retaining billed usage from physical records.
- Read CodeBuddy SDK transcripts and fork/clear records, explicit Grok chat
  snapshots, Hermes SQLite reasoning fields, and OpenCode's older multi-file
  JSON storage with `ImportSource::File` provenance.
- Add native Codex search projections and regression coverage for the 26
  existing clients. Extend fuzzing to their snapshot import paths.
- Expand opt-in real-client E2E tests from three to nine CLI entry points; retain
  failures and native evidence. Verify a real Zed GUI task and fix raw JSON tool
  inputs being incorrectly imported as missing arguments.

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
