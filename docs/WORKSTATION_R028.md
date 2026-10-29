# IndianOS R028 — Developer Workstation 20-workstream batch

**Base main:** `3215814bca8bfbae7e117963df0ce172db537425`  
**Coordinator:** `integration/workstation-r028-twenty`  
**Merge policy:** 20 independently scoped Git branches, one initial integration commit and one combined multi-workflow CI run; squash merge to one R028 main commit **only after full checks pass**.

| Agent | Feature | Functional implementation |
|---|---|---|
| 01 | Transactional folder-tree moves | `rust/kernel_core/src/workstation_r028/agent_01.rs`, with two focused regression tests |
| 02 | Per-file version history and optimistic restore | `rust/kernel_core/src/workstation_r028/agent_02.rs`, with two focused regression tests |
| 03 | UTF-8/BOM/binary file-format inspector | `rust/kernel_core/src/workstation_r028/agent_03.rs`, with two focused regression tests |
| 04 | Real project Rust use-dependency index | `rust/kernel_core/src/workstation_r028/agent_04.rs`, with two focused regression tests |
| 05 | Line-change source diff review | `rust/kernel_core/src/workstation_r028/agent_05.rs`, with two focused regression tests |
| 06 | Atomic multi-file editor patches | `rust/kernel_core/src/workstation_r028/agent_06.rs`, with two focused regression tests |
| 07 | Unicode-aware source line/column navigation | `rust/kernel_core/src/workstation_r028/agent_07.rs`, with two focused regression tests |
| 08 | Actual-source lexical completion index | `rust/kernel_core/src/workstation_r028/agent_08.rs`, with two focused regression tests |
| 09 | Source whitespace and CRLF cleanup | `rust/kernel_core/src/workstation_r028/agent_09.rs`, with two focused regression tests |
| 10 | Injection-resistant typed terminal command parser | `rust/kernel_core/src/workstation_r028/agent_10.rs`, with two focused regression tests |
| 11 | Terminal output search and pagination | `rust/kernel_core/src/workstation_r028/agent_11.rs`, with two focused regression tests |
| 12 | Authenticated typed terminal job lifecycle | `rust/kernel_core/src/workstation_r028/agent_12.rs`, with two focused regression tests |
| 13 | Real-source build-input fingerprint change analysis | `rust/kernel_core/src/workstation_r028/agent_13.rs`, with two focused regression tests |
| 14 | Dependency-aware build batch scheduler | `rust/kernel_core/src/workstation_r028/agent_14.rs`, with two focused regression tests |
| 15 | Rust test/assertion source inventory | `rust/kernel_core/src/workstation_r028/agent_15.rs`, with two focused regression tests |
| 16 | Trusted conditional debugger breakpoints | `rust/kernel_core/src/workstation_r028/agent_16.rs`, with two focused regression tests |
| 17 | Owner-scoped debugger execution trace | `rust/kernel_core/src/workstation_r028/agent_17.rs`, with two focused regression tests |
| 18 | Collision-safe keyboard shortcut router | `rust/kernel_core/src/workstation_r028/agent_18.rs`, with two focused regression tests |
| 19 | Light/Dark/Crystal typed workstation profiles | `rust/kernel_core/src/workstation_r028/agent_19.rs`, with two focused regression tests |
| 20 | Cross-module real-RamFs QEMU guest acceptance | `rust/kernel_core/src/workstation_r028/agent_20.rs`, with two focused regression tests |

## Verification
- 40 focused Rust module tests (two per 20 workstreams), 2 integration tests and QEMU guest smoke.
- QEMU guest pass marker: `IndianOS: R028 real workstation project editor terminal build debug PASSED`.
- Exact-head gates: Roadmap SHA; S0 security; independent reviewer; Orbit/QEMU GUI; M1 reboot durability and VFS; M2 signed-app and network acceptance; complete x86_64 kernel/QEMU.
- Do not merge failed or pending full-kernel checks; any coordinator fixes are folded into one main commit by squash.

## Scope boundaries
File and source operations modify real *volatile RamFs* byte contents. Disk durability, installed native compiler, full hardware debugger, fully rendered graphical IDE and signed OTA updates are **not** implemented by these modules. Build and debugger state APIs require trusted external executors; source dependency/completion indexes are lexical rather than compiler-semantic. These are 20 Git workstreams, **not** 20 simultaneously running independent autonomous agents.
