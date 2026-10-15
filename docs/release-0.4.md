# 0.4.0 local release candidate — 2026-10-15

This candidate is **not published**. The version in Cargo.toml identifies a local
package for review. No consumer repository has been upgraded, and these checks do
not certify 26 real clients or a production migration.

## Upgrade boundary

0.4.0 is a conservative minor-version boundary for changed default output and
snapshot semantics. `Event` was already `#[non_exhaustive]` in 0.3.0: adding
`Event::Content` does **not** break compliant external exhaustive matches. However,
`EventKinds::ALL` now includes Content, and serialized event streams may contain a
new variant. Old deserializers do not necessarily accept it. Explicitly choose
projections if the consumer needs only messages, tools or usage.

Content retains native thinking, signatures, media references and context controls.
It may overlap a text/tool projection; do not count it as another message or call.
Structured tool output is retained in `ToolResult.output`. Source records remain
available through the raw reader when every original byte is required.

Kimi, Pi and CodeBuddy snapshot imports now apply the documented branch, clear,
undo and compaction rules before returning visible history; billed usage remains
attached to physical records. Consumers needing physical history should use the
stream/raw APIs instead. Grok snapshots, OpenCode file provenance, and Zed's raw
JSON tool inputs are also included; none of these changes was in the 0.3.0 tag.

To evaluate the packaged candidate, unpack `agent-sessions-0.4.0.crate` and use a
path dependency on its `agent-sessions-0.4.0` directory. After a separately authorized
registry release, consumers choosing the new behavior must change their dependency
to `agent-sessions = "0.4"`; a `"0.3.0"` requirement does not select 0.4.

## Verification scope

All checked-in fixtures remain synthetic. A new example accepts **one explicit
file**, captures its bytes once in memory and independently projects raw JSON. It
compares user/assistant text, call IDs/names/arguments, result IDs/raw output/text,
selected non-text content and physical record offsets. It prints counts only:

```sh
CARGO_BUILD_JOBS=2 cargo +1.95.0 run --locked --release --example verify_native_history -- claude /path/to/session.jsonl
CARGO_BUILD_JOBS=2 cargo +1.95.0 run --locked --release --example verify_native_history -- codex /path/to/session.jsonl
```

Codex uses explicit `ResponseItems`; this comparison does not certify paginated
`CompletedItems` selection, usage accounting or every native content variant.
An unsupported input or mismatch exits unsuccessfully. Unknown records must be
investigated, not made supported merely by ignoring them.

Read-only local historical samples, with no model calls or new tasks:

| Client version from source | Records / bytes | Text / calls / results / selected Content | Comparison | Complete support |
|---|---:|---:|---|---|
| Claude 2.1.295 | 562 / 4,450,264 | 238 / 74 / 74 / 57 | All selected fields and offsets match | **No**: 4 artifact-comment-monitor and 4 artifact-autoreact-ledger records remain unknown |
| Codex 0.162.1 | 1,274 / 3,528,739 | 75 / 144 / 144 / 115 | All selected fields and offsets match | Yes for this sample and selected mode |

No transcript, private path, ID or tool payload was added to the repository or
verification logs. These are existing-history import checks, **not new-task E2E**.
Previous real-client observations remain in [support.md](support.md).

The separate consumer smoke check uses the unpacked candidate, outside this
crate. Its usage-only and metadata/message calls follow the current ccstats
`src/source/session_reader.rs` and `src/session_details.rs` API shapes (ccstats
currently requests 0.2.2 locally). It also exercises Content serialization, Pi
branch replay and Zed raw JSON input. This is an external-crate API check, not
building/running ccstats, proving independent adoption, cost parity or migration.

## Snapshot memory comparison

The patch changes only buffering after the existing bounded raw framer. Replay-only
CodeBuddy, Pi, Kimi and Grok no longer build an unused contiguous copy. Document-only
sources no longer retain every RawRecord. Gemini still keeps both representations
because legacy JSON detection can fall back to native JSONL replay. Streaming
Claude/Codex and database paths are unchanged. Limits, final status, errors and
source provenance remain governed by the existing framer/parser.

A macOS arm64 / Rust 1.95 release-build comparison against `ecf696583` used three
fresh processes per build and fixture, `/usr/bin/time -l` maximum resident set
size. The machine was shared, so these figures are local observations, not a
portable guarantee or a throughput benchmark:

| Fixture | Input bytes | Baseline median RSS | Candidate median RSS | Reduction |
|---|---:|---:|---:|---:|
| Pi linear journal, 24,576 messages | 102,088,704 | 502,661,120 | 400,441,344 | 20.3% |
| Cline pretty JSON array, 24,576 messages | 101,400,579 | 481,755,136 | 342,409,216 | 28.9% |

Every run returned exactly 24,576 messages and supported=true. RSS remains much
larger than the file: parsed JSON, replay state and returned events still require
memory. The file limit is not a process-memory limit.

Generate identical public synthetic inputs (no real conversation data):

```sh
python3 - <<'PY'
import json
message = 'x' * 4096
with open('pi-large.jsonl', 'w') as f:
    for _ in range(24576):
        f.write(json.dumps({'type':'message','message':{'role':'user','content':message}}, separators=(',', ':')) + '\n')
with open('cline-large.json', 'w') as f:
    f.write('[\n')
    for i in range(24576):
        f.write(json.dumps({'role':'user','content':message}, separators=(',', ':')) + (',\n' if i < 24575 else '\n'))
    f.write(']\n')
PY
CARGO_BUILD_JOBS=2 cargo +1.95.0 build --locked --release --example inspect_session
/usr/bin/time -l target/release/examples/inspect_session pi pi-large.jsonl
/usr/bin/time -l target/release/examples/inspect_session cline cline-large.json
```

Run each measurement three times, then repeat in a separate checkout at
`ecf696583066546af9c3c7c504cf8f701bf19a4d`, using the same generated inputs.
On Linux use `/usr/bin/time -v` and account for its RSS unit (KiB).

Release gates: all-target tests, doctests, fmt, clippy with warnings denied,
verified cargo package, and the external consumer smoke check. Local receipts
record exact commands, package hash and commit. Cross-platform/MSRV CI, a registry
publication and downstream application migration remain separate gates.
