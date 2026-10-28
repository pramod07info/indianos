# IndianOS R020 — 20 major developer-workstation workstreams / one build

- Base main: `b33b33d0dfa1bac5e4436c9bcd37075007fdc839`
- Integration branch: `integration/workstation-r020-twenty`
- Delivery: twenty real scoped development branches, each with a substantial source file, multiple APIs, and three focused tests, assembled as one integration-source commit.
- This is **not** an assertion that twenty separate autonomous AI workers were launched.
- No automatic merge unless exact-head Rust, S0, full kernel QEMU and M2 acceptance pass.

## Delivered technical workstreams

| Agent | Subsystem | Scope |
|---|---|---|
| 01 | project-snapshot | Authenticated RAM project checkpoint/restore with revision guard |
| 02 | editor-line-model | Editor line/column mapping, indent, deletion, byte-preserving navigation |
| 03 | undo-redo-history | Multi-level undo/redo with stale revision, bounded history and redo invalidation |
| 04 | refactor-preview | Atomic symbol refactoring across real project files with rollback |
| 05 | fs-rename-batch | Atomic multi-file copy/move/rename via checksum-validated RAM snapshot |
| 06 | workspace-profiles | Desktop/Developer/Server/All-in-One editions with service RAM/CPU quotas |
| 07 | task-run-dag | Dependency build DAG with deadlines, explicit outcomes and failure propagation |
| 08 | diagnostic-filter | Compiler diagnostics feed, dedupe, job filtering, per-owner privacy |
| 09 | artifact-manifest | Reproducible artifact provenance matching compiler, flags, source digest |
| 10 | debug-watch | Supervisor-authenticated conditional debugger watchpoints |
| 11 | debug-timeline | Monotonic debugger trace with owner-bound event history and replay protection |
| 12 | version-control | Version-control ancestry planner and reviewer-gated fast-forward metadata |
| 13 | patch-engine | Atomic multi-file exact-preimage patch engine |
| 14 | shortcut-router | Permission-bound keyboard command router and focus lifetime |
| 15 | window-layout | Actual kernel window table three-pane terminal/editor/debug layout |
| 16 | keyboard-compose | UTF-8 source typing, scalar-safe backspace and bounded composition |
| 17 | secure-clipboard | Generation-bound clipboard with TTL and zeroization |
| 18 | settings-export | Versioned checksum-protected workstation preference export/import |
| 19 | session-revocation | PID-generation-scoped session lifecycle and resource accounting |
| 20 | integration-smoke | Full guest-safe integration of edits, tasks, profiles, windows, clipboard and restore |

## Single build acceptance

- Compile all twenty `rust/kernel_core/src/workstation_r020/agent_XX.rs` modules in the main Rust crate, host and `no_std` configuration.
- Run each module's three focused positive/adversarial Rust tests, plus two cross-module tests.
- On `test-r041-workspace` QEMU mode, exercise `agent_20::smoke()` and require `IndianOS: R020 developer workstation session-edit-build-windows PASSED` in serial output.
- Preserve standard full kernel/QEMU boot, M1 durable VFS, M2 native-app acceptance, S0 security, independent review and SHA-validation gates at the exact new commit.
- Compare source base against `main` before merge; no forced merge on red CI.

## Safety and honest product boundaries

These are substantial, bounded **code primitives** and real RAM-backed file/window integrations, not a finished VS Code-compatible GUI IDE, native compiler, remote Git client, debugger trap handler, hardware GPU renderer or physical-block durable project workspace. Text positions are byte-oriented; UTF-8 scalar deletion is not whole grapheme-cluster input. Checkpoint and project refactor operate on RamFs, not durable VFS; actual disk installation/rollback remains separate. No OS is claimed 100% malware-proof.
