# IndianOS R034 — 20 developer workstation functions

Baseline: merged R033 #600 at `2562bfb2a7c01eb8ee2f5d3183bcbfe7839d9264`.

Twenty independent native Rust modules for the following capabilities:

1. owner-scoped recent projects: `rust/kernel_core/src/workstation_r034/agent_01.rs`
2. navigation history: `rust/kernel_core/src/workstation_r034/agent_02.rs`
3. hierarchical explorer grouping: `rust/kernel_core/src/workstation_r034/agent_03.rs`
4. line range selection: `rust/kernel_core/src/workstation_r034/agent_04.rs`
5. wrapped find-next: `rust/kernel_core/src/workstation_r034/agent_05.rs`
6. symbol lookup: `rust/kernel_core/src/workstation_r034/agent_06.rs`
7. safe drag destination: `rust/kernel_core/src/workstation_r034/agent_07.rs`
8. save conflict policy: `rust/kernel_core/src/workstation_r034/agent_08.rs`
9. activity ledger: `rust/kernel_core/src/workstation_r034/agent_09.rs`
10. file-type filters: `rust/kernel_core/src/workstation_r034/agent_10.rs`
11. owner-scoped command history: `rust/kernel_core/src/workstation_r034/agent_11.rs`
12. output paging: `rust/kernel_core/src/workstation_r034/agent_12.rs`
13. terminal quota: `rust/kernel_core/src/workstation_r034/agent_13.rs`
14. dependency cycle detection: `rust/kernel_core/src/workstation_r034/agent_14.rs`
15. incremental change filter: `rust/kernel_core/src/workstation_r034/agent_15.rs`
16. test result summaries: `rust/kernel_core/src/workstation_r034/agent_16.rs`
17. debug timeline: `rust/kernel_core/src/workstation_r034/agent_17.rs`
18. debugger run/pause transitions: `rust/kernel_core/src/workstation_r034/agent_18.rs`
19. window focus/visibility: `rust/kernel_core/src/workstation_r034/agent_19.rs`
20. combined feature smoke: `rust/kernel_core/src/workstation_r034/agent_20.rs`

## Integration and limitations

Each module has a regression test; agent 20 is the integrated acceptance invoked by the existing kernel `test-r041-workspace` smoke path. One coordinator PR/combined build. Follow all exact-head CI checks and squash merge to one main commit only on success.

This batch consists of bounded state algorithms and model primitives, NOT twenty separately executing AI coding agents, not a fully wired/interactive graphical workstation, and not real native compiler/debugger backends. Persisted state and userspace UI integration need follow-on implementation.
