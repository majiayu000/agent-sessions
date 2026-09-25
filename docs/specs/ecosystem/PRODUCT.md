# Ecosystem compatibility contract

Status: implementation contract, 2026-09-25. Extends v0.1 without changing
its strict snapshot, explicit usage ledger or occurrence-preservation guarantees.
Release candidate: 0.2.0 (neither 0.1 nor 0.2 has been published at this point).

## Raw records

Archive consumers need exact bytes, source byte offsets, independent continuation
from a known offset, and all record types including unrecognized or malformed JSON.
Expose a bounded raw reader separate from typed Events. It performs no JSON repair,
normalization, UTF-8 coercion or content filtering. Each delivered record retains
its delimiter. A consumer owns whether a delimiter-free final record is committed.
Raw completion means IO completion, not JSON/schema validation.

Raw file reads seal a byte length at open; generic reads can require an exact
boundary. Premature EOF, IO failure and budgets are observable. Start offsets
label the position of an already-positioned BufRead; file helpers perform seek.
Line numbers count records within this read. Byte offsets are absolute.

## History

Claude history: display text, timestamp in milliseconds, sessionId, project.
Codex history: text, ts in seconds, session_id. Text/time/session ID may be absent:
keepline historically counts a ts-only object as one history entry. Objects with
invalid supplied field types error by default; missing optional fields are None.
An explicit lenient field policy preserves legacy JSON-entry counters and exposes
invalid field names without inventing values. Blank lines
are skipped. Physical duplicate entries remain distinct. History entries are not
complete transcripts or automatically unique sessions. No pastedContents payload
is exposed as conversational content. Consumers preserve their date/count policy.

## Titles and previews

Read Claude projects/*/sessions-index.json summary and Codex session_index.jsonl
thread_name, only for requested session IDs. Preserve origin SourceSummary versus
SourceTitle, append-order last-nonempty semantics, missing-index behavior, and
privacy-safe index errors. Never substitute prompt text for an absent title.

ccp continues to display the first text block of the first usable user message,
whitespace flattened and capped by its existing preview policy. Message exposes
text segment byte ranges and first_text(); it still preserves joined text for
existing consumers. Early stopping/prefix budget is not a complete-file claim.

## Verification

Raw byte/offset resume, unknown and malformed rows, CRLF/UTF-8, exact boundaries,
short snapshots, and partial tails have focused tests. History tests distinguish
milliseconds/seconds, repeated IDs and timestamp-only entries. Title tests reject
malformed indices without leaking values and prove no prompt fallback. Existing
v0.1 golden/contract/MSRV/clippy checks remain required.
