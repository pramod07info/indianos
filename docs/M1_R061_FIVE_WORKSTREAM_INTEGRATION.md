# IndianOS M1/R061 — Five meaningful workstation workstreams, single integration commit

This implementation is based on latest main `918bf3c2828172c69a9d2c4699375706bee0e7cb`, including merged PR #677 (R060).

| Workstream | Real change | Regression tests |
| --- | --- | --- |
| RAM Files transaction | `RamMutation` Put/Remove/Move applied through one version-checked shadow filesystem; late failures roll back all changes | 3 |
| Durable developer source editor | Unicode-safe line duplication, undo/redo, COW save and actual remount; rejects phantom lines and stale owners | 3 |
| Desktop window layout | Owner-verified 2-D grid tiling with pixel-exact remainder distribution and all-or-nothing geometry publication | 2 |
| Terminal command editing | Reverse substring history search retaining unsent draft, plus caret-aware character transposition | 4 |
| Persistent Terminal file operations | `slice /src offset count /dst` extracts real binary byte ranges via authenticated, non-clobbering checksummed filesystem create | 3 |

15 new Rust regression tests across 5 existing production Rust modules, plus this document.
No synthetic agent_* placeholder files were created. Separate independent Codex
coding processes were **not** launched from this GitHub connector; these are
5 non-overlapping functional workstreams implemented by one coordinator.

Changes extend host-testable APIs and a persistent mounted-disk command but do
not claim a production normal-boot ring-3 desktop, compositor, or a newly closed
M1 acceptance gate. RAM Files is not reboot-persistent.

## Acceptance
- Exact PR-head Rust host/no_std compilation, M1 combined, M2, and S0 security
- Actual QEMU boot and GUI Files/Terminal persistent reboot tests
- Independent review, all required GitHub checks, no test suppression
- Leave PR DRAFT until every required test passes
- No physical disk formatting or host hardware writes

CI results must be confirmed independently before reporting acceptance.
