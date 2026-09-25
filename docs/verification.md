# Local verification — 2026-09-25

## Completed

* Rust 1.88.0 check and all-target tests: 36 integration tests passed. The one
  ignored env_child helper is intentionally executed twice by the subprocess
  environment test; it is not an untested environment path.
* 25 synthetic transcript fixtures compared against full event/provenance/summary
  golden snapshots. Hand-written contract tests independently assert accounting,
  errors, completeness and projection isolation.
* README rustdoc example passed.
* fmt check and clippy with warnings denied passed on 1.88.0 and 1.95.0.
* cargo package verification compiled the distributable crate. Missing public
  repository/homepage metadata emits a warning because no remote was created.
* AddressSanitizer/coverage-instrumented cargo-fuzz runs used nightly
  1.100.0-nightly and cargo-fuzz 0.13.2. An initial run completed 1,137,705 inputs
  in 61 seconds without a crash. After projection changes, another 60-second
  run completed with exit 0. This is finite smoke evidence, not a proof of no panic.

Commands: cargo fmt --check; cargo check --locked; cargo test --locked --all-targets;
cargo test --locked --doc; cargo clippy --locked --all-targets -- -D warnings;
cargo package --allow-dirty --locked. rust-toolchain.toml selects MSRV 1.88.0.
1.95.0 was also used for current-toolchain checks.

## Real-data reads

All scans were read-only, local, and aggregate-only. The source files were not
copied into fixtures and no production memory database was opened or changed.
Live sessions continued growing, so byte/event totals from different scans are
not directly comparable. Elapsed time was observed under ongoing machine activity,
not a controlled performance benchmark.

| Check | Result |
|---|---|
| Claude default-limit scan | 135 files, 96,142,093 bytes; all complete, no parsing errors |
| Codex legacy mode, explicit 16 MiB line cap | 6,529 files, 7,126,259,788 bytes; all complete, no errors or unknown types |
| Codex response mode, explicit 16 MiB line cap | 6,529 files, 7,126,485,536 bytes; all complete; 1,932 response usage events, no errors or unknown types |
| Independent remem text-projection comparison | 6,664 files; 169,260 user/assistant occurrences; zero different or incomplete files |

The independent comparator mirrors remem 7f4e144f raw_transcript.rs extraction:
physical ordinal, role, exact text, and epoch timestamp. It reads an exact captured
byte prefix per file. It does not compare remem identity-ledger writes, filters,
database replay, or a full remem executable. See examples/compare_messages.rs.

The first scans found three format/limit details:

* tool_search_output is a tool catalog, not a textual tool result. It is explicitly
  counted as known-ignored; a synthetic fixture now covers its shape.
* Claude tool_reference is explicitly counted as known-ignored; fixture added.
* One Codex row was 10,903,579 bytes. Default 8 MiB correctly returns TooLong.
  Full scans explicitly requested 16 MiB, rather than weakening the default or
  hiding the error. The product has not promised that every real row fits defaults.

## Delivery boundary

Implemented: standalone crate, revised spec, fixtures, reader/discovery contracts,
two usage modes, diagnostics, scanner/comparator examples, CI configuration and
local package verification.

Pending: running CI on a remote, publishing, replacing consumer dependencies,
ccstats cost/tool/report parity and performance gates, remem isolated DB replay,
refine provider-adapter parity. Library tests and text parity do not certify those
consumer migrations. Original research PLAN.md was retained without edits.
