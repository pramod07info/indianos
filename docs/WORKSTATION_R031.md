# IndianOS Developer Workstation R031 — 20 substantial workstreams

**Merged main baseline:** `4c219f85732835931e59719e8d24a246a9f3f454` (R030 PR #594).  
**Coordinator:** `integration/workstation-r031-twenty`; one integrated source commit, one PR, one combined exact-head CI validation. Main receives one commit if squash merged after full green.

## Changes in real kernel Rust source

| Workstream | Functionality |
|---|---|
| 01 | Atomic project folder clone: recursively copies actual RamFs files, fails without partial publication on collisions or insufficient capacity. `rust/kernel_core/src/workstation_r031/agent_01.rs` |
| 02 | Recoverable directory trash and restore: atomic subtree moves with bounded token and project-path checks. `rust/kernel_core/src/workstation_r031/agent_02.rs` |
| 03 | Transactional cross-file source identifier rename with exact revision preconditions. `rust/kernel_core/src/workstation_r031/agent_03.rs` |
| 04 | Versioned filesystem snapshot capture, integrity validation, optimistic restore. `rust/kernel_core/src/workstation_r031/agent_04.rs` |
| 05 | Real project changed-file inventory against captured revision baseline. `rust/kernel_core/src/workstation_r031/agent_05.rs` |
| 06 | Rust editor lexing: code, nested comments, escaped string-literal spans. `rust/kernel_core/src/workstation_r031/agent_06.rs` |
| 07 | Atomic multi-cursor UTF-8 edit with preimage and overlap validation. `rust/kernel_core/src/workstation_r031/agent_07.rs` |
| 08 | Conservative whitespace cleanup retaining CRLF/LF and stale revision detection. `rust/kernel_core/src/workstation_r031/agent_08.rs` |
| 09 | Live TODO/FIXME source navigation with exact location and invalidated stale links. `rust/kernel_core/src/workstation_r031/agent_09.rs` |
| 10 | Per-user source bookmarks tied to real file generation and UTF-8 character boundary. `rust/kernel_core/src/workstation_r031/agent_10.rs` |
| 11 | Authenticated bounded terminal command batch on live guest files with all-or-nothing response output. `rust/kernel_core/src/workstation_r031/agent_11.rs` |
| 12 | Owner-scoped terminal scrollback ring with complete-line eviction. `rust/kernel_core/src/workstation_r031/agent_12.rs` |
| 13 | Sandbox-scoped shell working directory navigation based on actual directory contents. `rust/kernel_core/src/workstation_r031/agent_13.rs` |
| 14 | Terminal job queue with IDs, owner isolation, verified finish, cancellation and deadlines. `rust/kernel_core/src/workstation_r031/agent_14.rs` |
| 15 | Non-executing terminal variable-template expansion from owner-scoped environment. `rust/kernel_core/src/workstation_r031/agent_15.rs` |
| 16 | Trusted incremental build evidence cache invalidated on source changes. `rust/kernel_core/src/workstation_r031/agent_16.rs` |
| 17 | Fail-closed release permit requiring trusted diagnostics, test ledger and matching source hash. `rust/kernel_core/src/workstation_r031/agent_17.rs` |
| 18 | Authenticated monotonic debugger event trace with bounded retention and pagination. `rust/kernel_core/src/workstation_r031/agent_18.rs` |
| 19 | Pixel-exact editor/terminal pane resizing, minimum widths and geometry validation. `rust/kernel_core/src/workstation_r031/agent_19.rs` |
| 20 | Whole batch guest acceptance across project, editor, terminal, build, debugger and window services. `rust/kernel_core/src/workstation_r031/agent_20.rs` |

## Verification and merge gate

- 40 focused Rust regression tests (2 per workstream) plus two cross-module integration tests in `mod.rs`.
- `test-r041-workspace` guest smoke invokes `workstation_r031::agent_20::smoke()` and emits `IndianOS: R031 workstation real source editor terminal release debug PASSED` only after success.
- Exact-head `cargo test --offline`, `no_std` build, kernel/QEMU, S0, M1, M2, Orbit preview and independent review are required; do not merge any failed or pending build.
- Source commits are assembled on the coordinator and squash-merged into one main commit after checks. Do not bypass required checks.

## Honest capability boundaries

- Actual guest `RamFs` bytes are read and changed, but these operations do **not** independently persist after reboot; they require integration with durable VFS.
- IDE lexical tools are not semantic Rust compiler APIs. The release workflow consumes trusted build/test evidence; it does not compile applications itself. The debugger trace requires a separate trusted runtime debugger.
- Workstreams are **20 substantial source implementations**, not proof of 20 concurrently executing independent AI coding agents; repository workflow must support that separately. The guest smoke is an opt-in kernel test path, not proof of full desktop UI wiring.
