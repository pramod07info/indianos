# IndianOS M1/R057 — three functional workstation subsystems, one commit

**Base:** `c0afbe6be403aa42ef39477c4d2887b155725950`, R056 verified merged. The three code changes below modify existing kernel_core Rust sources. No synthetic agent files.

| Workstream | Deliverable | Regression tests |
| --- | --- | --- |
| Terminal search | Literal `grep <word> /path` over the authenticated RAM-backed workspace. Full-file read, bounded 512-byte result, every matching line returned once in source order, read capability required; malformed path, permission denial, missing file, overflow or short caller output never publish partial content. Search does not mutate the RAM volume. `help` advertises available commands. | Real multiple-line results/no-match/read-only state; denied generation/invalid path and result overflow; insufficient destination/missing file |
| Command editor | Atomic, non-overlapping `replace_all_literal(needle, replacement)` in `LineEditor` and the corresponding `InputLine` wrapper. Printable ASCII only, validated capacity, no heap or syscalls, no mutation on invalid input or expansion beyond 192 bytes, preserve clipboard; caret moves to end after successful replacement. | Multiple replacements and deletion, capacity/error invariants, InputLine clipboard remains intact |
| Graphical workstation | `DamageQueue::transfer_batch_with_damage` coordinates existing fail-atomic Broker multi-window reassign with pending-frame cleanup. Rejecting the entire input/grant batch leaves graphics work unchanged; successful recovery deletes stale work for all reused window IDs, preserving other windows' damage and input order. Surfaces still require separate trusted compositor lifecycle management. | Two-window transfer with peer preservation; failed batch retains all redraw and event state |

**New regression tests:** 8 in three existing source modules.

**Scope:** Terminal grep and editor are authenticated process-side bounded helpers, not a complete POSIX shell or IDE. The search backend here is temporary RAM storage; it does not imply real disk persistence. The graphics primitive does not launch a ring-3 compositor. No hardware or data-destructive operation is requested.

**Agents:** 3 distinct engineering workstreams implemented by the connected coordinator, **0 independently launched Codex agents**. The GitHub connector cannot spawn autonomous coding worktrees. No false agent counts.

**Acceptance:** Exact-head GitHub compilation, Rust unit tests, no_std/security S0 regression, combined reliability tests, QEMU boot and persistent Files/Terminal, desktop screenshots, independent review, and mandatory branch protection. Do not merge until required validations are confirmed. No local Rust compiler is installed in this connected session.
