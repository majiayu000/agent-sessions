# Ecosystem delivery lanes

## Intent contract

- Goal: complete the approved shared parser and all ecosystem consumer migrations, preserve application policies and original user worktrees.
- Done when: all scoped code, focused and required full verification, durable commits and delivery evidence exist; registry/publication and remote gates reported separately.
- Mode: execute_direct. Merge policy: no_merge pending explicit merge authorization.
- Remote truth: required before remote delivery; local implementation currently truth level C. CI source: repository workflows.
- Capability: native collaboration tools available; explicit parallel authorization received for the six remaining repositories.
- WIP budget: six native lanes, matching the authorized six-repository parallel scope; dependent remem/archive API work stays read-only until core gate opens.
- Shared state: no writes to user HOME, original checkouts, hooks or global config. Cargo shared target may serialize builds; each repository has one full-suite owner.

## Ownership

| Lane | Write scope | Dependency / verification |
|---|---|---|
| core | agent-sessions implementation and tests; quotabar isolated worktree | Own shared API changes; preserve title error contract; verify and commit T4 |
| remem | isolated remem only | Inspect first; request core APIs; implement after API gate; own full suite and isolated smoke |
| refine_ccp | isolated refine and ccp, serial within lane | Existing 0.2 API stable at 8c00aa2; own per-repo checks |
| archive | isolated chat-archive-rs only | Existing raw API; request directory discovery from core; own full suite |
| keepline | isolated keepline only | Existing history API; own required checks |
| looper | isolated life-looper and ccstats, serial | Extend opt-in session details; rebase upstream pricing; own full Go and ccstats suites |
| coordinator | PLAN.md, WORKLOG.md, THREADS.md; delivery evidence | Integration, independent review and final remote/registry gates; no overlapping worker edits |

Workers must read applicable instructions and Rust skills, preserve operation error contracts, add meaningful regression coverage, commit verified steps, and report exact SHAs/checks. No pushes, publication or merges by workers. High-context instructions/configuration are excluded. Native IDs and results are appended after dispatch.

## Native dispatch evidence

All six lanes spawned via collaboration.spawn_agent: /root/core, /root/remem, /root/refine_ccp, /root/archive, /root/keepline, /root/looper. Result collection pending. No merge or publication delegated. User original worktrees retain pre-existing changes.

## Refresh and dependency findings

- GitHub owner authenticated as majiayu000. Empty public agent-sessions repository created and origin configured; no source pushed or crate published yet. Crates.io name lookup returned 404.
- Consumer remotes refreshed. ccstats origin/main advanced to 9274c1e (0.8.1 pricing repairs); looper lane owns integration before details API. life-looper origin/main advanced to 9815bc4 (changes outside cost scanner); same lane owns rebase. Other recorded consumer bases unchanged.
- QuotaBar T4 Rust suite now 132 passed / 5 ignored after library title wording fix 2361ee7; pre-existing strict clippy warnings tracked separately.
- Exact six authorized repositories became five consumer lanes by grouping refine then ccp, plus one shared API lane to prevent competing edits.

## Remote delivery ledger

- Core source3ae0828 published as agent-sessions0.2.0; CI36156978344 all six jobs passed. GitHub release v0.2.0 exists.
- Refine PR229 at15677a8: implementation4c3793b plus minimal existing-baseline RustSec patch rustls0.23.45; refreshed CI pending.
- Ccp PR11 at511be68: registry dependency, local and remote CI passed.
- Archive PR29 at23bfdaa: registry dependency, local29tests and no-patch locked check passed; no remote CI workflow exists.
- Remem feature issue1088; specdbf45024 fullpreflight using required1.97, separate isolated worktree. Runtime preflight follows.
- QuotaBar396e29d adds candidate changelog after6412318 migration; registry ccstats gate still pending.
- Keepline7fb4c7d waits final ccstats SDK window.
- ccstats0a939c6 had997tests green; latest scoped details fixes pending Rust gates. Looper5a5e363 Go scope/coverage tests/premerge green, final real CLI gate pending.

Current local Cargo scheduling is serialized: remem owns full spec/runtime preflight; then ccstats/desktop, keepline, final release CLI and paired throughput. Other lanes may run read-only/Go/Node checks. New native independent review collected concrete defects and fixes; no merge was performed.
