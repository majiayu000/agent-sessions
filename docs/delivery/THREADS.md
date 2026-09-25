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
| looper | isolated life-looper only | Existing ccstats CLI; request missing SDK output from coordinator; own Go checks |
| coordinator | PLAN.md, WORKLOG.md, THREADS.md; ccstats only if required | Integration, independent review and final remote/registry gates; no overlapping worker edits |

Workers must read applicable instructions and Rust skills, preserve operation error contracts, add meaningful regression coverage, commit verified steps, and report exact SHAs/checks. No pushes, publication or merges by workers. High-context instructions/configuration are excluded. Native IDs and results are appended after dispatch.

## Native dispatch evidence

All six lanes spawned via collaboration.spawn_agent: /root/core, /root/remem, /root/refine_ccp, /root/archive, /root/keepline, /root/looper. Result collection pending. No merge or publication delegated. User original worktrees retain pre-existing changes.
