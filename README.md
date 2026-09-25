# agent-sessions

Discover and stream Claude Code and Codex session files as typed Rust events.
Local v0.2 implementation, integrated with ccstats and QuotaBar in migration
branches. Other consumer migrations are in progress. Registry publication is pending.

```toml
[dependencies]
agent-sessions = { path = "../agent-sessions" }
```

```rust
use agent_sessions::{Agent, Event, ReadOptions, read_from};
use std::io::Cursor;

let input = r#"{"type":"user","message":{"content":"hello"}}"#;
let mut reader = read_from(Agent::ClaudeCode, Cursor::new(input), &ReadOptions::default())?;
for event in reader.by_ref() {
    let event = event?;
    if let Event::Message(message) = event.value {
        assert_eq!(message.text, "hello");
    }
}
let summary = reader.finish();
assert!(summary.is_complete());
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Discovery and events

`Roots::from_env()` honors `CLAUDE_CONFIG_DIR` and `CODEX_HOME`, falling back to
the platform home directory. `Roots::from_home()` or explicit `Roots` isolate
tests and custom installations. Empty environment overrides return an error.

`discover()` scans Claude `projects` and Codex `sessions` / `archived_sessions`;
returns files and individual discovery errors; skips symlinks. Missing roots are
normal. Subagent path classification is only path evidence: Codex metadata may
identify subagents in ordinary session paths. Inspect Meta events when applying
subagent filtering. `SessionFile::inspect()` supports a hook-supplied host/path.

Events: `Meta`, `Message`, `ToolCall`, `ToolResult`, `Usage`. Each is wrapped in
`Located` with timestamp, session/message ID when present, physical ordinal,
byte range and event index. Text preserves whitespace and repeated occurrences;
multiple text blocks are joined with a newline. Thinking/image contents are not
exported. Tool arguments remain JSON values or raw strings; no implicit repair.
This is a text/usage/tool projection, not a lossless multimodal transcript format.

## Usage accounting

Counters are `Option<u64>`: absent differs from zero. Negative, fractional or
malformed counters are errors. `Usage.semantics` tells you how counts overlap:

| Host | input | output | cache_write_1h |
|---|---|---|---|
| Claude | excludes cache categories | native output | subset of cache_write |
| Codex | includes cache read/write | includes reasoning | normally absent |

`TokenCounts::exclusive()` produces disjoint input/output only when the required
components are known; it returns None for inconsistent subtraction. It never
assumes absent components are zero. Cost, model aliases, synthetic-message policy,
and cross-file deduplication belong to consumers.

Codex usage is selected explicitly with `ReadOptions.codex_usage`:

* `TokenCount` (default): skip identical cumulative vectors, prefer last sample,
  otherwise derive deltas; preserve totals and derivation basis.
* `Response`: emit `token_usage_record` usage, with response ID when available.

Never sum both modes. There is no auto-switch halfway through a stream. A
single-pass reader cannot retract older emitted events when a newer ledger
appears later. Claude message and Codex response IDs are provided as dedup keys,
but repeated source records remain observable. Caller dedup scope must include
the intended host/session/source identity. Legacy Codex usage without IDs carries
cumulative evidence and physical provenance, not a fabricated global ID.

## Reading and failure contracts

Default limits are 200 MiB per file and 8 MiB per physical line (including newline).
Limits are checked during reading, including generic streams; oversized lines
are drained without buffering their full contents. `stop_at_byte` requires an
exact captured prefix; an input shorter than that prefix fails. Length alone
does not detect concurrent rewrites: immutable snapshot verification is a caller
responsibility. Readers start at record zero; arbitrary mid-file cumulative
usage needs a separate baseline protocol and is not supported.

Recoverable line errors are yielded as `StreamError::Line`. Continue or abort
according to your application's policy. Fatal IO/limit/snapshot errors are
yielded once and stop the iterator. A malformed record invalidates cumulative
continuity; the next cumulative-only usage restores its baseline with an error.
Meta messages are retained, including those with `isMeta`; consumers choose filters.

Strict tail parsing is default. `TailMode::AllowIncomplete` tolerates only a final
unterminated JSON value whose parser error is EOF. Invalid UTF-8, malformed final
JSON, or an incomplete JSON record ending with a newline remain errors.

Always call `finish()` after iteration. It does not drain unread data:

| Status | Meaning |
|---|---|
| Complete | clean end, all selected events delivered |
| CompleteWithErrors | end reached after line errors |
| IncompleteTail | append tail was tolerated |
| StoppedEarly | caller did not consume to the end |
| Failed | fatal stream error |

Only Complete authorizes a whole-snapshot commit. Unknown record types are
counted separately and may require an additional application completeness gate.
`last_complete_byte` marks only the contiguous error-free, fully delivered prefix;
it is not a license to advance an application cursor after failure.

`EventKinds` selects projections while retaining deterministic event indexes.
Excluded payloads are not projected or semantically validated: malformed usage cannot
block a message-only consumer, and a usage-only reader does not require content.
Relevant metadata continuity and top-level diagnostics remain active; content
diagnostics cover inspected projections. Event slots are 0=Meta, 1=Message,
2=Usage, 3+content-block-index=tools (Codex standalone tools use 3).
Known irrelevant records are
counted separately from unknown types. Summaries never contain message text.

## Development

Rust 1.88+; synchronous IO; no network or database dependency.

```sh
cargo +1.95.0 fmt --check
cargo +1.95.0 clippy --all-targets -- -D warnings
cargo +1.88.0 test --locked --all-targets
cargo +1.95.0 test --doc
cargo +1.95.0 run --release --example scan -- claude legacy
cargo +1.95.0 run --release --example scan -- codex response 100
cargo +1.95.0 run --release --example scan -- codex legacy 100000 16
cargo +1.95.0 run --release --example compare_messages
```

The scanner reads local files and prints aggregate diagnostics only. Checked-in
fixtures are synthetic; no private transcripts are copied into the repository.
The fourth scan argument explicitly raises the line limit in MiB. One observed
local record was 10.9 MB, above the default 8 MiB limit. Do not silently discard
limit errors. `compare_messages` checks the pinned remem text projection only,
not database replay, identity-ledger behavior or the complete remem application.
See [product contract](docs/specs/v0.1/PRODUCT.md),
[technical contract](docs/specs/v0.1/TECH.md) and
[format notes](docs/formats.md). Existing PLAN.md is historical planning, not
evidence of completed migrations or publication.

## Ecosystem APIs (0.2)

`read_raw_file` / `read_raw_from` preserve exact record bytes, delimiters and
absolute offsets. They do not parse JSON, discard unknown records or validate
UTF-8. Raw completion describes IO only; archive consumers retain their own
commit/hash/partial-record policy. `RawReadOptions.start_offset` permits resuming
from an established byte boundary. Generic sources must already be positioned.

`read_history` / `read_history_from` read Claude and Codex history with explicit
millisecond/second timestamp units. Entries are not unique sessions. Missing text
and IDs remain None. Strict field checking is default; a consumer preserving old
record-counting behavior may explicitly select `strict_fields: false`, which
reports malformed optional fields in `HistoryEntry.invalid_fields`.

`load_session_titles` reads only native title indices for requested IDs. It never
uses message bodies as fallback titles. `Message::first_text()` preserves the
first original text block; `text_segments` are UTF-8 byte ranges in joined text.
A title-preview consumer may stop iteration early or use a bounded prefix source;
that does not mean a complete snapshot was read.

## Statistical normalization and provenance

The default AccountingPolicy::Strict rejects negative counters and malformed TTL
breakdowns. UsageStatistics is an explicit policy for existing statistics clients:
negative Claude counters are clamped, cache-TTL subsets capped, empty usage objects
remain observable, and cumulative decreases use saturating deltas. Every affected
Usage carries adjustments. Missing native fields still remain None; callers own
whether their aggregation treats them as zero. Codex invalid cache buckets are
not clamped. This policy also accepts legacy Claude usage-only envelopes.

Located.timestamp_text retains native timestamp spelling, independently of parsed
UTC time. record_id (Claude UUID) and message_id (provider message ID) are distinct.
ToolCallKind distinguishes client functions, custom calls and server tools.
Roots::from_env_for resolves only one host, avoiding unrelated override failures.

For Codex statistical projections, the reader borrows envelope fields and skips
unrequested message/tool bodies before materializing values. Selected records use
the same semantic decoder. Framing reuses its input buffer and uses vectorized
newline search; raw consumers still own the exact bytes of each returned record.

## Archival and transcript projections

`discover_directory` shares the recursive walker for explicit archive roots and
reports missing roots. It excludes descendant `subagents` directories when requested;
explicit roots stay eligible even under a directory with that name.

`project_conversation` and `tolerant_timestamp_epoch` retain archival consumers'
lenient field policy, including empty messages and first-present timestamp priority.
`project_codex_function` borrows native arguments/output without coercing JSON types.
`project_transcript` supports legacy Codex envelopes, role-specific text blocks,
source metadata and meta-message tags. These helpers operate on existing JSON values;
callers still validate JSON syntax, incomplete tails and commit boundaries. Typed
stream readers retain strict validation by default.

History entries also expose `timestamp` in native units even outside the datetime
range. `HistoryReader::next_line_no()` identifies a failed physical I/O line,
including preceding blank lines. Valid UTF-8 whitespace-only records are skipped.

Statistical readers count nonzero Codex last-usage records lacking cumulative
totals under `ReadSummary::ignored_types[CODEX_MISSING_TOTAL_USAGE]`. This is an
observable accounting gap; no guessed usage is emitted and stream error semantics
remain unchanged. Consumers requiring complete cost attribution should reject it.
