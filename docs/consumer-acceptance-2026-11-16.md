# 0.4 consumer acceptance — 2026-11-16

The first checkpoint below is retained as history for the rusqlite 0.37 / Rust
1.88 package. See [active ccstats follow-through](#active-ccstats-follow-through)
for the follow-through's Rust 1.89 / rusqlite 0.41.1 candidate. Do not apply
the initial dependency failure or the retired CCP result to a different package.

## Initial decision: keep active-consumer acceptance pending

CCP is an existing consumer whose README says it is superseded by Fibo. Its
successful check establishes bounded compatibility with real application handlers,
not an active migration or new adoption. The active ccstats consumer cannot
currently resolve this candidate's native SQLite graph. **Do not publish 0.4 on
the strength of the retired consumer's result.**

The decision under test is whether an existing application can use the prepared
candidate without losing previews or changing its token ledger. Stop on native
dependency conflicts, unexplained projection differences, or any need to change
credentials, a daily installation or the consumer's database architecture.

## Tested identities

| Object | Identity |
|---|---|
| Library runtime source | `babace58d638efa116258b13d7a51d5c2c1199ef` |
| Verified package | `agent-sessions-0.4.0.crate`, SHA-256 `29542b889f75d31565710df8bb589f383751885137ed9b28847bc1b0fae6b1bc` |
| ccstats baseline | `784c9112ac4e9661426659fb9512579b5e0f0d53`, 0.12.0, locked reader 0.2.2 |
| CCP baseline | `94a61acc537b04c652f600208c67e005e60149ef`, locked reader 0.2.0 |
| CCP regression commit | `1a757ce610cb9343137cee2b3d746d8aeb06d5c4` |
| Local toolchains | macOS arm64; library/package Rust 1.88.0; CCP Rust 1.95.0; ccstats baseline Rust stable |

The package was verified before this note was added. This documentation commit
does not change runtime source, but a subsequent archive including the note will
have different bytes and must not reuse the package hash above. All work used
isolated worktrees. Temporary package-path dependencies and lockfile changes were
restored, not committed as production upgrades.

## ccstats: active upgrade blocked before compilation

The unchanged baseline passed its existing `cli_claude`,
`cli_codex_source_routing`, `cli_session_details` and `tools_integration` tests.
These execute the actual CLI and its usage/details/tool paths, not API shapes
copied into a new smoke crate.

Changing only the reader dependency to the unpacked verified package made
`cargo metadata --format-version 1 --offline` fail:

```text
agent-sessions 0.4.0 -> rusqlite 0.37.0 -> libsqlite3-sys 0.35.0 (links=sqlite3)
ccstats 0.12.0      -> rusqlite 0.41.1 -> libsqlite3-sys 0.39.1 (links=sqlite3)
```

Cargo permits only one package with a given native `links` identity. This is
neither a parser regression nor a token-parity failure: the candidate cannot be
built in this consumer's current graph. No SQLite dependency was downgraded and
no database support was removed to manufacture a pass.

rusqlite 0.41.1 declares Rust 1.89 while this library promises Rust 1.88. Simply
matching ccstats therefore also requires an explicit MSRV decision. Resolving
that dependency/MSRV constraint is separate work; an active ccstats migration
has not passed.

## CCP: real existing raw/projection consumers remain compatible

The actual `/api/profiles/default/sessions` and `/api/profiles/default/usage`
handlers call CCP's `sessions::scan` and `usage::scan`. Tests exercised the real
router with disposable profile directories, the existing in-memory secret store
and an injected launcher. No live model, Keychain access, HTTP listener, new chat
or external provider was used.

Both the locked 0.2.0 baseline and the unpacked 0.4.0 package passed all 65
existing application tests plus the new production-handler regression: **66
passed, zero failed, one explicitly ignored private-history test**. That regression
checks the first visible text preview with image/thinking/tool blocks present,
no repeated preview, one usage contribution per replayed assistant ID, and exact
input/output/cache token buckets. The ignored check was invoked separately below.
Format and warning-denying Clippy checks passed for the tested configurations.

Scope matters: session previews use `read_raw_from` plus `project_transcript`;
usage uses the raw reader and CCP's own JSON accounting. Neither consumes the
new default `Event::Content` stream or Pi/Kimi/CodeBuddy snapshot replay API.
Passing these routes does **not** validate those new semantics in a downstream
application. Library replay regressions are separate evidence. No snapshot
feature was added to CCP merely to satisfy an acceptance label.

### Existing private history, counts and fingerprints only

One existing 1,990,216-byte Claude transcript was selected through a disposable
symlink, never copied into either repository. Both production handlers ran
against that same file. After each run the original bytes, length and mtime were
checked unchanged. No source path, real session ID, prompt, tool payload or
credential appears in the public test or JSON receipts.

Final baseline/candidate receipts match: one resumable session, its 27-character
preview projection, the session projection fingerprint including cwd/preview,
the complete usage response fingerprint, all four token totals, one usage
day/model and the source SHA-256. Fingerprints do not expose the private values.

The first comparison found different complete session-response fingerprints.
Those original receipts were retained. `DirEntry::metadata` sees the newly
created fixture symlink's own mtime, which necessarily differs between runs.
The final comparison excludes only that fixture-generated `modified` field;
source bytes/mtime are independently checked unchanged. This is not timestamp
parity for arbitrary symlink inputs or a general normalization framework.

`DefaultHasher` fingerprints are same-compiler local differential signals, not
cryptographic attestations or a stable format. The local runner separately
records the input SHA-256. Only counts and hashes leave test memory.

## Commands and local receipts

The regression is CCP's `tests/session_reader_acceptance.rs`. Run against the
baseline, then a temporary dependency on the unpacked verified package; restore
the manifest and lockfile after the latter:

```sh
cargo +1.95.0 test --locked --all-targets
cargo +1.95.0 clippy --locked --all-targets -- -D warnings
cargo +1.95.0 fmt --all --check

# Optional: explicitly select one existing, bounded historical file.
CCP_ACCEPTANCE_TRANSCRIPT=/path/to/existing/session.jsonl \
  cargo +1.95.0 test --locked --test session_reader_acceptance \
  existing_transcript_through_production_handlers -- --ignored --exact --nocapture
```

Raw local receipts live under each isolated worktree's ignored
`target/consumer-acceptance-20261116/` directory:

- Library: `package.log`, `library-tests.log`, `library-doctests.log`,
  `library-clippy.log`; archive under `target/package/`.
- ccstats: `baseline-tests.log`, `candidate-dependency-resolution.log`;
  original manifest/lockfile restored and source working tree unchanged.
- CCP: `baseline-tests.log`, `candidate-tests.log`, `final-baseline-tests.log`,
  configuration-specific Clippy logs, and `baseline-existing-history.json` /
  `candidate-existing-history.json`. `*-first-run.*` retains the fixture-mtime
  mismatch. No private transcript text is stored in those receipts.

Library Rust 1.88 checks passed: 160 all-target tests, zero failures, ten ignored;
one doctest; warning-denying Clippy; format; and verified `cargo package`.
Windows/Linux runtime checks, downstream CI and publication were not run here.

Verified: the packaged candidate preserves bounded raw/projection behavior in
CCP's actual handlers with the existing input unchanged. Unverified: an active
consumer upgrade and downstream Content/snapshot replay semantics. Adjacent
case: active ccstats is blocked by SQLite linking and the accompanying MSRV
choice. Keep the 0.4 release decision pending until that constraint is resolved.

## Active ccstats follow-through

Decision: adopt the minimal candidate dependency/MSRV alignment and accept the
bounded active-ccstats upgrade compatibility result. Release remains a separate
owner action after the actual Rust 1.89 gate and exact-artifact checks; the
`rust-version` field alone does not prove that gate.

This follow-through deliberately aligns the unpublished 0.4 candidate to Rust
1.89 and rusqlite 0.41.1. Published 0.3.0 remains Rust 1.88. No ccstats database version,
project feature layer, compatibility shim or application installation changed.
Only rusqlite and its necessary transitive lock graph changed. The fuzz lock
also now correctly identifies the path package as 0.4.0 rather than 0.3.0.

rusqlite 0.41 made `FromSql` for `u64` opt-in. The first compilation therefore
reported nine missing trait implementations at existing database size reads.
Enabling the upstream dependency's `fallible_uint` feature preserves their
checked conversions without editing the database readers or introducing casts.
The new `sqlite_length_contract` regression accepts zero and `i64::MAX`, rejects
negative and out-of-range numeric representations, and checks nullable sizes.
Existing read-only database, row-budget and replay regressions also run normally.

The active application is still ccstats 0.12.0 at
`784c9112ac4e9661426659fb9512579b5e0f0d53`, with only the regression in
`58f2fed53342727a1edc44c2c81a1585d19ec195` added. Its production database
dependency remains rusqlite 0.41.1. A temporary dependency on the unpacked new
candidate resolves to exactly one libsqlite3-sys 0.39.1, shared by the library and
the application. Neither the private package path nor its lockfile is committed.

### Active application observations

- The existing CLI `cli_claude`, `cli_codex_source_routing`,
  `cli_session_details` and `tools_integration` groups, plus the new content
  projection case, pass: 54 tests, zero failures. The production binary executes
  normal statistics, details, tool aggregation, deduplication and source routing.
- The added case includes image/thinking/text/tool blocks and a repeated assistant
  usage sample. Actual details retain one visible prompt, two requests, 17 input
  and four output tokens; daily totals remain 21 and the Read tool counts once.
  It passes with both the original 0.2.2 and the packaged candidate.
- The same existing 1,990,216-byte historical file also goes through the actual
  CLI's `daily`, `session --details` and `tools` commands with offline pricing and
  caches disabled. The complete canonical JSON hashes match baseline/candidate
  for all three reports: one session, zero parse errors, 93 requests and 95 tool
  calls. Only counts/hashes are retained; prompt text and private paths remain
  in process memory. Tool summary rows are sorted by name before hashing their
  set-like list; no values are discarded.
- Discovery intentionally rejects symlinks. For this paired application check a
  single private snapshot is therefore created in a mode-0700 temporary directory
  outside all repositories, with a mode-0600 file. The same snapshot path is used
  by both binaries; it is removed after final artifact verification. Original
  bytes, size and mtime are checked unchanged. No conversation body is written
  into a repository, package or receipt log.

This establishes compatibility of the active consumer's existing typed
usage/meta/message/tool projections. It does not mean ccstats now consumes
`Event::Content` or Pi/Kimi/CodeBuddy snapshots. Their library regressions are
separate evidence; adding unwanted consumer features is not a release condition.

### Software gates and final artifact binding

Rust 1.95 local checks pass: all-target check, 161 tests with zero failures and
ten explicit ignores, one doctest, warning-denying Clippy, format, verified
package, and the fuzz manifest's locked dependency build. Actual Rust 1.89 is
also required, either locally or through the exact-head CI minimum-toolchain job;
record its compiler and platform instead of extrapolating from the 1.95 run.

The follow-through uses `target/ccstats-followthrough-20261116/` in the isolated
library and consumer worktrees. This is separate from the original receipts.
The first unsigned-read compilation failure, focused regression, library gates,
candidate CLI tests, the shared SQLite tree and paired history JSON receipts are
retained there. The final clean package is created after the source/documentation
commit. `final-artifact.json` outside the tracked source binds its exact Git head,
archive SHA-256, compiler and final active-consumer checks, avoiding a document
that tries to include its own commit or package hash.

The first package's hash above is historical and must not identify this new
candidate. Final key CLI regressions and the same-history comparison are rerun
against the clean archive; a successful preliminary package is not substituted.
No tag, registry publish, consumer release or daily application upgrade is part
of this acceptance work.
