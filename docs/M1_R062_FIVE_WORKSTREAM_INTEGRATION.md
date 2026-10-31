# IndianOS M1/R062 — Five real workstation functions, one commit

Base: `9abdb4dbbf60e79fae0e3599b920225315d293ee` (merged PR #680). This changes **five existing Rust production modules** and adds a single documentation file.

| Workstream | Real implementation | New Rust tests |
|---|---|---:|
| RAM Files | Versioned, all-or-nothing batch copying across multiple existing files, rejecting stale source versions and duplicate destinations | 3 |
| Source editor | Unicode-safe delete of complete 1-based source lines, one undo/redo step, COW save and disk remount verification | 3 |
| Graphical desktop | Owner-verified window cascade with atomic coordinates, epochs and screen-bound checking | 2 |
| Live Terminal input | Atomic multi-key edit batches with no hidden Submit, no partial text/caret updates on late failures | 3 |
| Persistent filesystem Terminal | `concat /first /second /dest` reads two real binary COW files and creates a checked, non-clobbering output | 3 |

**14 new Rust tests, five distinct features, one consolidated commit, one PR, no placeholder agent files.** 
This GitHub connection does not launch five independent Codex coding sessions. The changes represent five separately scoped coordinator-implemented workstreams (actual independent worker processes: zero).

## Acceptance before merge

Run exact-head Rust host tests, no_std build, M1 combined, S0 security, QEMU normal boot and reboot persistence, reviewer and all repository-required checks. Leave PR in draft until CI passes. A passing source-level regression does not establish a completed M1 gate or working graphical app without actual guest acceptance.
