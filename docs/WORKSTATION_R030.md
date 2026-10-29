# R030 — Developer Workstation: 20 functional workstreams, single integration commit

**Base verified main:** `e6b45e287cac4ffab8d090de8e0477b9fe7116c4` (merged R029 PR #592).  
**Coordinator branch:** `integration/workstation-r030-twenty`  
**Integration policy:** one complete Git commit containing all workstreams, one PR to main, run combined existing required CI, merge only if exact head green and reviewed.  

## Implemented source and regression tests

| Workstream | Functional implementation |
|---|---|
| 01 | Deterministic real-file project inventory and revision listing — `rust/kernel_core/src/workstation_r030/agent_01.rs` |
| 02 | Project disk-usage and quota enforcement on actual guest RAM files — `rust/kernel_core/src/workstation_r030/agent_02.rs` |
| 03 | Revision-aware no-overwrite source copy within authorized project — `rust/kernel_core/src/workstation_r030/agent_03.rs` |
| 04 | UTF-8 byte-to-line/column cursor mapping and inverse conversion — `rust/kernel_core/src/workstation_r030/agent_04.rs` |
| 05 | Atomic revision-checked global find/replace in actual guest source — `rust/kernel_core/src/workstation_r030/agent_05.rs` |
| 06 | Exact identifier rename preserving similarly named symbols — `rust/kernel_core/src/workstation_r030/agent_06.rs` |
| 07 | Complete-line source viewport previews with bounded output — `rust/kernel_core/src/workstation_r030/agent_07.rs` |
| 08 | Quoted-string-aware bracket nesting validation — `rust/kernel_core/src/workstation_r030/agent_08.rs` |
| 09 | Rust function outline and byte/line navigation index — `rust/kernel_core/src/workstation_r030/agent_09.rs` |
| 10 | Bounded non-recursive glob search over project file paths — `rust/kernel_core/src/workstation_r030/agent_10.rs` |
| 11 | Project-scoped terminal path resolution with traversal prevention — `rust/kernel_core/src/workstation_r030/agent_11.rs` |
| 12 | Real log-file tail command with whole-line boundaries — `rust/kernel_core/src/workstation_r030/agent_12.rs` |
| 13 | Owner-isolated bounded environment variables without shell eval — `rust/kernel_core/src/workstation_r030/agent_13.rs` |
| 14 | Source-byte and compiler-version build fingerprinting — `rust/kernel_core/src/workstation_r030/agent_14.rs` |
| 15 | Topological build job scheduling and cycle detection — `rust/kernel_core/src/workstation_r030/agent_15.rs` |
| 16 | Trusted compiler diagnostic severity report and release veto — `rust/kernel_core/src/workstation_r030/agent_16.rs` |
| 17 | Monotonic owner-scoped verified test outcomes — `rust/kernel_core/src/workstation_r030/agent_17.rs` |
| 18 | Trusted debugger value-change watch and freshness checks — `rust/kernel_core/src/workstation_r030/agent_18.rs` |
| 19 | Spatial keyboard focus movement among workstation panes — `rust/kernel_core/src/workstation_r030/agent_19.rs` |
| 20 | End-to-end guest smoke combining live project, editor, terminal, build, debugger, and window logic — `rust/kernel_core/src/workstation_r030/agent_20.rs` |

Each feature is a Rust module with 2 focused unit tests (40 module tests total), plus 2 integration tests in `mod.rs`. The QEMU workspace guest feature `test-r041-workspace` also calls the R030 end-to-end smoke and logs `IndianOS: R030 developer workstation real-file integration PASSED` on success.

## Scope and security

- **Real data paths:** modules 01–03, 05–07, 12 and 14 operate on actual in-guest `RamFs` file bytes or metadata, not fake file names. Source edits/copy require matching per-file generations, and project prefix checks use path-component boundaries.
- **Fail closed:** bounded buffers, no shell execution in environment/path utilities, owner+trusted evidence required for test and debugger state, dependency cycles block build dispatch.
- **Limits:** this is kernel-side workstation logic and acceptance, not full installed Rust compiler, graphical IDE, network shell, native debugger, or durable on-disk filesystem. Lexical outline/bracket/rename tools are not full syntax-aware Rust refactoring; edits within string literals are not yet excluded. The guest RAM filesystem alone loses content on reboot.
- **Agents:** workstreams describe 20 development scopes; GitHub source files or branches do **not** imply 20 concurrently executing coding AI agents. The repository's existing `agent-swarm.yml` uses a consolidated ten-module verification job and is not a 20-agent execution service.
- **CI:** `cargo test --offline --manifest-path rust/kernel_core/Cargo.toml`, freestanding Rust build, existing QEMU kernel and security gates on PR; confirm R030 QEMU marker via `test-r041-workspace`. Do not mark merged/passed until GitHub checks confirm.
