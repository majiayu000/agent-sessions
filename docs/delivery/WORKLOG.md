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
