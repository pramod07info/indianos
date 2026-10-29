# R033 IndianOS Developer Workstation — 20 functional source workstreams

Based on merged R032 PR #598, main `248feb9735d35253a15a6834e3f86d04e4170823`. All changes are combined on coordinator branch `integration/workstation-r033-twenty` in one integration commit and PR. Squash-merge to one main commit only after CI and review.

| # | Implemented functionality |
|---|---|
| 1 | Revision-protected delete of actual project source file — `rust/kernel_core/src/workstation_r033/agent_01.rs` |
| 2 | Real-file content checksum and change detection — `rust/kernel_core/src/workstation_r033/agent_02.rs` |
| 3 | Source line count based on actual guest file bytes — `rust/kernel_core/src/workstation_r033/agent_03.rs` |
| 4 | Workspace per-file quota and overflow guard — `rust/kernel_core/src/workstation_r033/agent_04.rs` |
| 5 | Live CRLF-to-LF source normalization with stale revision rejection — `rust/kernel_core/src/workstation_r033/agent_05.rs` |
| 6 | Editor tab cursor pagination calculation — `rust/kernel_core/src/workstation_r033/agent_06.rs` |
| 7 | Unicode-safe UTF-8 character cursor conversion — `rust/kernel_core/src/workstation_r033/agent_07.rs` |
| 8 | Bounded undo-history stack with precise last-in-first-out — `rust/kernel_core/src/workstation_r033/agent_08.rs` |
| 9 | Source word/identifier occurrence scanning — `rust/kernel_core/src/workstation_r033/agent_09.rs` |
| 10 | Canonical project-root path boundary guard — `rust/kernel_core/src/workstation_r033/agent_10.rs` |
| 11 | Safe numeric terminal argument parser — `rust/kernel_core/src/workstation_r033/agent_11.rs` |
| 12 | Whole-line terminal scrollback tail window — `rust/kernel_core/src/workstation_r033/agent_12.rs` |
| 13 | Terminal job deadline / elapsed-time evaluation — `rust/kernel_core/src/workstation_r033/agent_13.rs` |
| 14 | Build job prerequisite bitmask and failure gate — `rust/kernel_core/src/workstation_r033/agent_14.rs` |
| 15 | Strictly monotonic build epoch validation — `rust/kernel_core/src/workstation_r033/agent_15.rs` |
| 16 | Uniquely indexed test-suite pass/fail aggregation — `rust/kernel_core/src/workstation_r033/agent_16.rs` |
| 17 | Debugger monotonic watch-event update and stale event guard — `rust/kernel_core/src/workstation_r033/agent_17.rs` |
| 18 | Debugger breakpoint address duplicate validation — `rust/kernel_core/src/workstation_r033/agent_18.rs` |
| 19 | UI workstation pane geometry and minimum-size validation — `rust/kernel_core/src/workstation_r033/agent_19.rs` |
| 20 | Integrated real RamFs, editor, terminal, test, debug and workspace smoke — `rust/kernel_core/src/workstation_r033/agent_20.rs` |

Unit regression tests live alongside every feature module. `agent_20::smoke()` tests the combined 19 capabilities against live kernel RamFs and guards, and the existing `test-r041-workspace` boot smoke runs it from `kernel_main.rs`. Require exact-head Rust host tests, freestanding kernel/QEMU, S0, M1, M2, Orbit UI preview, roadmap checks and review.

**Limits:** These are genuine Rust kernel-side primitives, not a fully surfaced IDE UI. RamFs changes are not reboot durable on their own; debug watches are event records, not a hardware debugger, and this batch does not execute 20 independent AI coding agents. Never describe them as such. Must integrate features into userspace views as follow-on work.
