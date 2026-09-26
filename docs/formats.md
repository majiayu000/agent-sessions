# Claude Code and Codex session JSONL format reference

These are implementation observations, not provider guarantees. Initial source
baselines: ccstats 2e2a766, remem 7f4e144f, refine 88ceb45. Local structure-only
inspection confirmed Codex 0.156.1 token_usage_record payload. Tests are synthetic.
The reference describes agent-sessions 0.2.0; provider formats may change.

[Crate and quick start](../README.md) · [API docs](https://docs.rs/agent-sessions) ·
[Synthetic fixtures](../tests/fixtures)

## File locations and discovery

Paths below are relative to each configured root. `Roots::from_env()` uses
`CLAUDE_CONFIG_DIR` / `CODEX_HOME`, or `~/.claude` / `~/.codex` by default.
Empty overrides are errors. `Roots::from_home()` supports an explicit home.

| Data | Claude Code | Codex |
|---|---|---|
| Session JSONL | `projects/**/*.jsonl` | `sessions/**/*.jsonl`, `archived_sessions/**/*.jsonl` |
| History | `history.jsonl` | `history.jsonl` |
| Title index | `projects/*/sessions-index.json` | `session_index.jsonl` |

Discovery skips symlinks and reports individual errors. Missing roots are normal.
A `subagents` path is evidence, but Codex subagent sessions also occur in ordinary
session directories: inspect metadata before applying an application filter.
See [roots](../src/roots.rs), [discovery](../src/discover.rs), and
[discovery tests](../tests/discovery.rs).

## Claude Code records

`user` / `assistant`: message.content is text or blocks. Text blocks are joined
with newlines; tool_use/server_tool_use and tool_result get separate events.
`message.id`, `sessionId`, `uuid`, `parentUuid`, isMeta/is_meta, isSidechain, cwd,
gitBranch, version, message.model and message.usage are retained as applicable.
Legacy progress tools are at data.message.message.content.

usage contains input_tokens, output_tokens, cache_creation_input_tokens,
cache_read_input_tokens and optional cache_creation.ephemeral_1h_input_tokens.
inference_geo == "not_available" maps heuristically to Native; "" to Proxy;
other values to Unknown. Raw inference_geo is retained; this is not authoritative
provider/billing evidence. No pricing or model name normalization is performed.

## Codex records

session_meta and turn_context provide provenance/model updates. Structured source
and thread_source subagent evidence outrank originator fallback. Source exec stays
Exec even with originator Codex Desktop; vscode is Ide. This describes execution
provenance, not a judgment about whether a human initiated a session.

The [classifier](../src/codex/origin.rs) checks `thread_source: "subagent"`
first, then recognized `source` values (including structured objects), then
`thread_source: "automation"`, and finally known `originator` names.
Unrecognized evidence yields `Unknown`. The
[README table](../README.md#why-share-a-parser) and
[contract tests](../tests/format_contract.rs) show conflicting-field examples.
Raw `source`, `thread_source`, and `originator` remain available to callers.

response_item.message provides role/content. Event-msg mirrors of user/assistant
messages are counted as ignored to avoid duplicate conversations. Function and
custom-tool calls retain raw arguments; result records retain call_id/output.
Reasoning and selected system events are intentionally ignored and counted.

event_msg.token_count.info provides total_token_usage and optional last_token_usage;
info:null can be a rate-limit-only update. Cache read aliases are accepted. Missing
counts remain unknown. token_usage_record.payload.usage is a per-response sample;
payload.response_id identifies it. Turn/thread totals in the same record are not
additional billable samples. Choose one usage ledger per read.

Unknown top-level types, response/event subtypes, roles and content block types
are counted. Invalid known fields produce line errors. Add fixtures before adding
a type to the known-ignored set; ignoring is an explicit compatibility decision.

Observed tool_search_output records carry a tools catalog rather than an output
string. They and Claude tool_reference blocks are known-ignored projections,
with explicit counters and synthetic fixtures. They are not ordinary tool results.

## Token usage: select one Codex ledger

| Native record | Counts to read | Identity / accumulation |
|---|---|---|
| Claude assistant `message.usage` | Input, output, cache creation and cache read | `message.id` is a provider message ID; `uuid` is a separate source record ID |
| Codex `event_msg` with `payload.type: "token_count"` | `payload.info.total_token_usage` and optional `last_token_usage` | Cumulative evidence; the reader skips identical total vectors and derives deltas when needed |
| Codex `token_usage_record` | `payload.usage` | Per-response sample; `payload.response_id` is retained when present |

Use `ReadOptions.codex_usage = CodexUsageMode::TokenCount` (the default) or
`CodexUsageMode::Response`. A read never switches ledgers automatically.
Summing both can count the same work twice. Turn/thread totals accompanying a
response sample are context, not additional response usage.

Claude input excludes cache categories. Codex input includes cache read/write,
and Codex output includes reasoning. Missing counters remain `None`, not zero.
The Claude one-hour cache subset is part of total cache creation. Call
`TokenCounts::exclusive()` only when the needed components are known; inconsistent
subtraction returns `None`. Cost, pricing, and cross-file deduplication belong to
the application. See [usage contracts](../README.md#usage-accounting) and
[Codex decoder](../src/codex).

## History and title indexes

| Host | History ID | History text | Native timestamp |
|---|---|---|---|
| Claude Code | `sessionId` | `display` | `timestamp`: integer milliseconds |
| Codex | `session_id` | `text` | `ts`: integer seconds |

A history row is an entry, not necessarily a unique session. Missing IDs/text
remain absent. Strict field checking is the default; the compatibility option
`strict_fields: false` records malformed optional fields in `invalid_fields`.
See the [history field decoder](../src/history/entry.rs).

Claude title indexes contain `entries` with `sessionId` and optional `summary`.
Codex title index lines contain `id` and `thread_name`.
`load_session_titles` reads only these indexes for requested IDs, with no prompt
body fallback. Missing indexes are allowed; malformed indexes produce errors.
See the [title reader](../src/titles.rs).

## Streaming, malformed records, and raw archives

Session files are appendable JSONL streams. The default typed reader limits are
200 MiB per file and 8 MiB per physical line, including its newline. A caller
must handle limit failures explicitly; a large record is not an empty session.

The default tail policy is strict. `TailMode::AllowIncomplete` tolerates only
an unterminated final JSON value whose parser error is EOF. Invalid UTF-8,
malformed JSON, or incomplete JSON followed by a newline still fails.
Line errors may be recoverable, but fatal IO, limit, and snapshot errors end the
iterator. Always inspect `finish()`: reaching an iterator's end alone does not
prove a complete snapshot. See [failure contracts](../README.md#reading-and-failure-contracts).

Typed events are text/usage/tool projections. Thinking and image contents are
not exported, and known ignored records differ from unknown records. For an
archive that needs every byte, use `read_raw_file` / `read_raw_from`: these keep
record bytes, delimiters, and offsets without JSON or UTF-8 validation.
The archive still owns hashing, incomplete-record handling, and commit policy.

## Compatibility evidence and contributing a format change

The [synthetic fixture corpus](../tests/fixtures) pairs JSONL input with expected
events. [Format-contract tests](../tests/format_contract.rs) cover provenance,
metadata retention, event indexes, and tool arguments. The
[verification notes](verification.md) record the original scope and limitations.
Historical local-sample counts in [the research plan](research/plan-v2.md) describe
that sample and the pinned consumer revisions, not an ecosystem-wide error rate.

For a new format, provide a minimal synthetic record, the provider version if
known, and the expected event or diagnostic. Keep record structure and relevant
field types, but remove private conversation text, credentials, and personal
identifiers. Unknown records should become known-ignored only after an explicit
compatibility decision with a fixture.
