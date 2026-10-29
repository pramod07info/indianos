# IndianOS R032 — Twenty Functional Developer Workstation Workstreams

**Base main:** `cbe2defe160b726b151a9b027b7c1c51178d64a3` (R031 PR #596 merged).  
**Coordinator integration branch:** `integration/workstation-r032-twenty`.  
**Policy:** one initial integration commit containing all modules; exact-head combined CI; squash merge as one main commit only after independent review and all gates succeed.

## Implemented features

| # | Real functionality |
|---|---|
| 01 | Atomic three-file Rust project scaffolding with snapshot rollback; `rust/kernel_core/src/workstation_r032/agent_01.rs` |
| 02 | Sorted actual-source file inventory with content checksums and revisions; `rust/kernel_core/src/workstation_r032/agent_02.rs` |
| 03 | Directory-to-directory project comparison by path and file content; `rust/kernel_core/src/workstation_r032/agent_03.rs` |
| 04 | Baseline project manifest verification and changed-file audit; `rust/kernel_core/src/workstation_r032/agent_04.rs` |
| 05 | Atomic UTF-8 source search/replace across all project Rust files; `rust/kernel_core/src/workstation_r032/agent_05.rs` |
| 06 | Line-level real-file source diff, additions and removals; `rust/kernel_core/src/workstation_r032/agent_06.rs` |
| 07 | Trusted compiler diagnostics bound to source revision and code location; `rust/kernel_core/src/workstation_r032/agent_07.rs` |
| 08 | Multi-tab editor session with dirty-save and close-protection; `rust/kernel_core/src/workstation_r032/agent_08.rs` |
| 09 | UTF-8 correct code selection/copy with stale-file detection; `rust/kernel_core/src/workstation_r032/agent_09.rs` |
| 10 | String/comment-aware code folding on source braces; `rust/kernel_core/src/workstation_r032/agent_10.rs` |
| 11 | Authenticated sorted terminal project-file listing; `rust/kernel_core/src/workstation_r032/agent_11.rs` |
| 12 | Permission-scoped multi-file terminal grep with matching lines; `rust/kernel_core/src/workstation_r032/agent_12.rs` |
| 13 | Authorized terminal file append using write capability and file revision; `rust/kernel_core/src/workstation_r032/agent_13.rs` |
| 14 | Build/test/package dependency DAG with verified steps and failed-dependency skips; `rust/kernel_core/src/workstation_r032/agent_14.rs` |
| 15 | Trusted artifact digest registry bound to source revision and build ID; `rust/kernel_core/src/workstation_r032/agent_15.rs` |
| 16 | Replay-safe test evidence registry for concrete source generation; `rust/kernel_core/src/workstation_r032/agent_16.rs` |
| 17 | PC/line debugger breakpoints with trusted frame and file revision checks; `rust/kernel_core/src/workstation_r032/agent_17.rs` |
| 18 | Four-pane workstation viewport geometry, panel ownership, and focus; `rust/kernel_core/src/workstation_r032/agent_18.rs` |
| 19 | Typed command palette for permitted file/build/test/debug actions; `rust/kernel_core/src/workstation_r032/agent_19.rs` |
| 20 | QEMU guest all-workstreams acceptance against real kernel RamFs; `rust/kernel_core/src/workstation_r032/agent_20.rs` |

Each module has two behavioral regression tests (40), plus two cross-module integration tests in `mod.rs`. The R032 guest acceptance is executed within the existing kernel `test-r041-workspace` smoke feature and will log `IndianOS: R032 workstation projects editor terminal build debug PASSED` on success.

## Verify before merge

- Rust host tests: `cargo test --locked --offline --manifest-path rust/kernel_core/Cargo.toml`.
- Rust freestanding cross build, kernel/QEMU, S0 Security Gate, M1 durable VFS, M2 signed package/network, Orbit UI QEMU preview, Roadmap SHA and independent reviewer gating.
- Fail-closed: project writes are staged in guest snapshots and committed only if all constraints pass; terminal operations require session-scoped access tickets and revocable rights; trusted test/build/debug evidence is explicit; no caller may silently overwrite stale source revisions.
- Merge only after all required exact-head checks are green. Squash merges one combined commit to main. The repository must be configured separately if 20 independently executing AI coding agents are desired.

## Scope limits

- `RamFs` is volatile. A durable backend must be wired into workstation services for reboot persistence; no durability is claimed by these modules alone.
- Diagnostic, artifact, debugger and build APIs consume trusted evidence, but do not implement a native compiler, symbolic debugger, or cryptographically signed binary.
- Geometry and command-palette are kernel-side models, not a complete drawn/interactive graphical IDE. Existing QEMU preview does not automatically render these new panes.
- Workstream count represents 20 implemented functional modules and tests, **not** 20 actual concurrently running AI coding processes.
