# R035 — 20 functional Developer Workstation workstreams

Baseline merged PR #601 commit `4ecf812c01dd812a04559612a41821031290d096`. Single coordinator PR; squash merge as one commit on main only after all required exact-head CI passes.

1. **Owner-locked project session and root access** — `rust/kernel_core/src/workstation_r035/agent_01.rs`
2. **Atomic project file creation** — `rust/kernel_core/src/workstation_r035/agent_02.rs`
3. **Source-revision-locked file copy** — `rust/kernel_core/src/workstation_r035/agent_03.rs`
4. **Source-revision-locked file rename** — `rust/kernel_core/src/workstation_r035/agent_04.rs`
5. **Editor save with stale generation checks** — `rust/kernel_core/src/workstation_r035/agent_05.rs`
6. **UTF-8-safe source range replacement** — `rust/kernel_core/src/workstation_r035/agent_06.rs`
7. **Transactional multi-file Rust symbol refactor** — `rust/kernel_core/src/workstation_r035/agent_07.rs`
8. **Owner-locked, live-revision editor tabs** — `rust/kernel_core/src/workstation_r035/agent_08.rs`
9. **Real project source tree enumeration** — `rust/kernel_core/src/workstation_r035/agent_09.rs`
10. **Real-file source search and byte offsets** — `rust/kernel_core/src/workstation_r035/agent_10.rs`
11. **Authenticated terminal read-only command batching** — `rust/kernel_core/src/workstation_r035/agent_11.rs`
12. **Authorized terminal guest log append** — `rust/kernel_core/src/workstation_r035/agent_12.rs`
13. **Real-source compiler diagnostic location validation** — `rust/kernel_core/src/workstation_r035/agent_13.rs`
14. **Trusted dependency DAG build runner** — `rust/kernel_core/src/workstation_r035/agent_14.rs`
15. **Build artifact digest and generation validation** — `rust/kernel_core/src/workstation_r035/agent_15.rs`
16. **Owner-checked trusted test-suite result ledger** — `rust/kernel_core/src/workstation_r035/agent_16.rs`
17. **Trusted revision-bound source debugger breakpoints** — `rust/kernel_core/src/workstation_r035/agent_17.rs`
18. **Bounded monotonic debugger trace ring** — `rust/kernel_core/src/workstation_r035/agent_18.rs`
19. **Four-panel developer workspace geometry** — `rust/kernel_core/src/workstation_r035/agent_19.rs`
20. **Cross-module guest source/editor/terminal/build/debug smoke** — `rust/kernel_core/src/workstation_r035/agent_20.rs`

## Verification

Each module contains an executable Rust regression test; `agent_20` verifies integrated guest workflows and is registered in the existing kernel `test-r041-workspace` smoke gate. Run native host tests, no_std build, Security S0, Orbit graphical QEMU, M1 persistence, M2 package/network, Roadmap SHA, and kernel boot before merging. Fix failed workflows on the same integration PR.

## Scope boundaries

The features operate on real in-memory RamFs and existing kernel services, but do not yet provide reboot-durable storage, a userspace-rendered graphical IDE, an independently running compiler/debugger or twenty concurrent autonomous coding agents. These 20 scopes are functional source workstreams coordinated through one integration commit, not twenty parallel AI developers.
