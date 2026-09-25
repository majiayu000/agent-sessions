# Delivery work log

## Baselines

- agent-sessions local core: 3c610ca (v0.1, unpublished)
- ccstats origin/main: 2e2a766
- remem origin/main: 7f4e144f
- refine origin/main: 88ceb45
- quotabar origin/main: 578667f
- chat-archive-rs origin/main: dbcae9d (fetched before worktree creation)
- ccp / keepline: fresh clones; record SHA before edits.
- life-looper: cee1953 (local main; existing uncommitted dev script excluded)

## T0

Full ecosystem authorization received. Independent worktrees/clones prepared.
Fixflow per-step commits apply; Rust best practices and project instructions apply.
The fixflow supplemental reference files were not present in the installed skill
package; its main workflow is sufficient and is followed directly.

No consumer code changed yet. Production data remains read-only.

## T1 completed

Implemented local 0.2.0: exact raw framing/resume; explicit history units and
lenient legacy field policy; native title indices; original first-text block
segments. Synthetic golden snapshots updated only for the added segment ranges.
Validation: Rust 1.88 fmt/check, 46 integration tests, 1 rustdoc example, clippy
with warnings denied, and a 60-second AddressSanitizer fuzz run including raw and
history readers all passed. Logs: /tmp/agent-sessions-t1-tests.log and
/tmp/agent-sessions-t1-fuzz.log. No consumer migrations included in this commit.

## T2 completed

ccstats candidate 0.9.0 upstreamed QuotaBar weekly-reserve SDK patch; Grok patch
was already upstream. Commit d3de1f3. Replaced an invariant expect with an error
return and preserved exact snapshot equality without floating-point lint bypasses.
Validation: cargo test (1,014 passed in 39 suites), clippy -D warnings, check and
fmt. Output: /tmp/ccstats-weekly-full.log.

Baseline is sealed outside repositories at .agent-sessions-delivery/baseline.
147 Claude files (including title indices), 6,534 Codex files; source replicas
are APFS copy-on-write copies, never uploaded. Baseline binary ccstats-before
was built from 2e2a766 and ad-hoc signed. baseline/run.py explicitly selects
Claude, uses isolated config/cache and validates JSON output. Final before run
median: 3.15 s, 5 repetitions, application cache disabled, OS page cache retained.
Earlier runs that inherited a legacy user source config were discarded.

T3 integration must preserve ccstats negative-token clamp contract (existing
negative_token_integration), missing counters/TTL normalization, raw timestamp
spelling, tool_use versus server_tool_use, and global dedup IDs. Core defaults
remain strict; any optional compatibility normalization must be explicit and
auditable, not silently change all readers.

## T3.1 consumer contract additions

Added explicit UsageStatistics accounting policy with observable adjustments,
retaining strict defaults. Native timestamp text and record UUID are separate
from parsed time/provider message IDs; ToolCallKind preserves client/server tool
distinction. Added per-host environment resolution and fixed empty-model fallback
selection. Validation: 51 integration tests plus fmt/check/clippy passed on 1.88.
These additions were required by existing ccstats integration contracts, not
new product behavior. Existing v0.1 strict error tests remain unchanged/passing.

## T3 parity / performance investigation

Initial consumer tests passed (985 tests after moving helper tests into the shared
crate). Native row comparison against the pinned original Claude parser passed
for all 135 transcript files; discovery list/order is identical. Temporary diagnostic
module was removed from the consumer source. Log: /tmp/ccstats-raw-diagnostic.log.

CLI comparison requires excluding pricing_cache_age_seconds and canonicalizing
session/tool tie order; floating sums use tiny numerical tolerance. Three Claude
day cost values also varied in the ORIGINAL 0.8 binary with fixed input/pricing:
12 single-thread runs produced two vectors, 4 and 8 times respectively. Artifact:
baseline/pricing-variation.json. This is pre-existing price alias overwrite order
behavior, not a transcript row difference. No pricing rules were changed for it.

Naive shared Value parsing failed the performance gate (19.29 s median vs 3.15 s).
Added borrowed selective Codex headers, skipped unrequested bodies, reusable typed
reader scratch buffers and memchr framing. Semantic differential tests plus
source/SDK/negative-counter tests pass; optimized release measurement is next.

## T3 implementation and local gates completed

ccstats now consumes agent-sessions for Claude/Codex discovery, usage, client tools
and native title indices. Keeps app model normalization, buckets, dedup keys and
SDK types. Interactive includes IDE. Response-only Codex files are supported by
an error-free legacy-ledger fallback; mixed ledgers are never summed.

Final six report comparisons (daily/session/tools, Codex daily/monthly/session)
matched after excluding cache age, stable tie ordering and negligible float sums.
Artifact: baseline/comparison-after-native.json (all zero differences).

Five paired alternating full Codex runs: old median wall 5.01 s, new 4.73 s
(ratio 0.944); median user CPU 6.45 s versus 6.68 s (+3.6%). Apple M1 Pro, 8 CPUs,
32 GiB RAM. Godot and other desktop apps were active; wall timing is noisy and
this is not an idle-machine universal speed claim. Both meet the 5% paired gate.
Artifact: baseline/paired-native.json. Application cache disabled, OS cache retained.

Fresh ccstats full tests, fmt/check/clippy passed; shared strict/statistics tests
and 60-second ASan fuzz (including selective statistics) passed. Consumer lockfiles
still use local Cargo patch resolution pending the publication step.

## Parallel consumer integration

Explicit user authorization enabled six native lanes; ownership and live gates are
in THREADS.md. Original user worktrees remain untouched. T4 QuotaBar committed
6412318 after 132 Rust tests passed (5 ignored), fmt, check, warnings-only clippy,
and prior 612 frontend tests/build/version checks. Strict clippy has five warnings
in untouched existing modules; it is not represented as clean. Library title error
wording compatibility fixed in 2361ee7.

Remote refresh found ccstats main advanced to 9274c1e with 0.8.1 fallback-price
repairs; migration branch rebased preserving those repairs (4439ec6 weekly SDK,
28db1e0 parser migration). The previous real-data comparison still proves the
parser change against its pinned baseline, not equal costs after new price tables.
Life-looper baseline updated to 9815bc4 before its cost adapter work.

The apparent extra Claude-Code-Monitor repository is the same GitHub repository
as keepline: claude-hub redirects to majiayu000/keepline (repository id1113014090).
No duplicate migration or PR is required.

Empty public agent-sessions repository created at the approved owner/name; source
and registry publication remain pending final API and review gates. Remem feature
issue1088 tracks spec-to-implementation delivery; no capability closure claimed.
