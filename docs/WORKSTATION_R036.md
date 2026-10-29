# IndianOS R036: 20 functional Developer Workstation scopes

Baseline: merged R035 PR #604 at `263084baa28653ad589c8a7bc728bc45c1aabaf2`.

1. **Real filesystem project inventory** — `rust/kernel_core/src/workstation_r036/agent_01.rs`
2. **Atomic multi-file project creation** — `rust/kernel_core/src/workstation_r036/agent_02.rs`
3. **Root-secured revision-fenced rename** — `rust/kernel_core/src/workstation_r036/agent_03.rs`
4. **Atomic staged project deletion** — `rust/kernel_core/src/workstation_r036/agent_04.rs`
5. **UTF-8 source editor patch** — `rust/kernel_core/src/workstation_r036/agent_05.rs`
6. **Owner-scoped editor tabs** — `rust/kernel_core/src/workstation_r036/agent_06.rs`
7. **Unicode-safe line/column navigation** — `rust/kernel_core/src/workstation_r036/agent_07.rs`
8. **Project-wide source search** — `rust/kernel_core/src/workstation_r036/agent_08.rs`
9. **Transactional multi-file source replacement** — `rust/kernel_core/src/workstation_r036/agent_09.rs`
10. **Real source revision checkpoint** — `rust/kernel_core/src/workstation_r036/agent_10.rs`
11. **Authenticated terminal command batch** — `rust/kernel_core/src/workstation_r036/agent_11.rs`
12. **Scoped terminal grep** — `rust/kernel_core/src/workstation_r036/agent_12.rs`
13. **Permission-controlled terminal append** — `rust/kernel_core/src/workstation_r036/agent_13.rs`
14. **Dependency-aware build runner** — `rust/kernel_core/src/workstation_r036/agent_14.rs`
15. **Source-bound artifact verification** — `rust/kernel_core/src/workstation_r036/agent_15.rs`
16. **Trusted source-revision test evidence** — `rust/kernel_core/src/workstation_r036/agent_16.rs`
17. **Trusted breakpoint evaluation** — `rust/kernel_core/src/workstation_r036/agent_17.rs`
18. **Monotonic debugger trace buffer** — `rust/kernel_core/src/workstation_r036/agent_18.rs`
19. **Four-pane owner-scoped layout** — `rust/kernel_core/src/workstation_r036/agent_19.rs`
20. **End-to-end real guest integration** — `rust/kernel_core/src/workstation_r036/agent_20.rs`

Each workstream includes regression tests; all are exercised in `agent_20::smoke()` and registered in `test-r041-workspace` guest path. Use combined GitHub CI (S0, M1, M2, Orbit GUI, kernel QEMU, roadmap). Merge only after all exact-head checks green, preferably squash to one main commit.

These 20 functional workstreams are kernel service modules, **not** independently executing AI agents. RamFs changes aren't persistent across reboot; layout is a model, not a complete rendered IDE; evidence consumers are not compilers, debuggers or cryptographic signers.
