# agent-sessions v0.1

Status: implementation contract, 2026-09-25.
This contract supersedes the API sketches in [PLAN.md](../../../PLAN.md). PLAN.md is
preserved as research/planning history; its local survey figures are not
independently established acceptance evidence.

## Behavior

Discover and read Claude Code and Codex JSONL files into messages, tool calls,
tool results, metadata and usage. The first slice is the independent library;
consumer migrations are separately verified changes, not implied by a passing
library suite. No publishing or installation is part of local implementation.

* Preserve meta messages, sidechains, empty text, whitespace, repeated messages
  and physical record identity. Consumers control filtering and persistence.
* Distinguish absent counts from explicit zero; retain source token semantics,
  cumulative evidence and the basis of any derived usage.
* Every event has a physical record ordinal, byte range and event index.
* Invalid JSON/known malformed records produce recoverable line errors. I/O,
  file size limits and premature snapshot EOF are fatal stream errors.
* An incomplete final JSON record is tolerated only in explicit append mode;
  the default is strict. A complete JSON value without a newline is valid.
* A summary distinguishes complete, complete-with-errors, incomplete-tail,
  stopped-early and failed. Only an error-free complete summary authorizes a
  whole-snapshot commit; the library never writes a cursor or database.
* Report unknown record/content types and known irrelevant types separately.
  Diagnostics do not include raw transcript text.

## Scope and decisions

Discovery honors explicit Roots first. Roots::from_env uses CLAUDE_CONFIG_DIR /
CODEX_HOME (empty overrides are errors), then the platform home directory.
Scan Claude projects and Codex sessions plus archived_sessions. Missing default
directories are normal; permission failures are returned. Do not follow symlinks.
Subagent file classification is based on path evidence; metadata can additionally
identify a subagent. Main means no subagent evidence in the path, not proof of
an interactive session. Consumers that exclude subagents must also inspect Meta.

Codex source/thread_source/originator evidence is preserved. Structured subagent
evidence wins, followed by known source, then originator fallback. Unknown source
values stay observable. Session aggregation is optional and must not hide raw
metadata updates. IDE is a separate source class; consumers may map it to their
existing interactive scope.

Codex usage mode is explicitly TokenCount (default) or Response. A single reader
never emits both ledgers. Auto-preference across a whole file would need a first
pass or delayed output, and is deferred. TokenCount skips identical cumulative
vectors, prefers last_token_usage, otherwise derives a delta. Counter regression
without a last sample errors and rebaselines; missing counters remain None.
Malformed records break cumulative continuity: the next cumulative-only sample
rebaselines with an error rather than inventing a delta. Model changes do not
reset session totals. Response mode exposes response_id for caller deduplication;
it does not keep an unbounded set of response IDs. Claude similarly exposes
message.id without deduplicating distinct raw occurrences.

Current token semantics: Claude input/output exclude cache/reasoning categories;
Codex input includes cached/cache-write input and output includes reasoning.
Cache write 1h is a subset of total cache write. Recorded totals remain separate.
Endpoint inference from inference_geo is a documented heuristic, with raw evidence.

## Acceptance

1. Synthetic versioned fixtures cover both hosts, both Codex usage modes,
   origin combinations, tools, meta messages, usage gaps and legacy progress.
2. Contract tests cover all terminal states, invalid UTF-8, chunk boundaries,
   line/file/snapshot limits, continuing after a malformed record and early stop.
3. Golden event outputs include provenance and None versus zero.
4. fmt, check, all-target tests, clippy and rustdoc pass; MSRV tested separately.
5. Read-only local scans emit aggregate metrics only. Real transcripts are never
   copied to git fixtures or used to mutate production memory stores.

Full consumer migration gates remain: ccstats token/tool/model/day parity plus
explained corrections, remem occurrence/text/time identity and isolated replay,
refine expected origin/isMeta differences. No claim of adoption before those pass.
