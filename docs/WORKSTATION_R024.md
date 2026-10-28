# IndianOS R024 — Twenty Developer Workstation implementation workstreams

- **Base merged main**: `6cfae98c3b133bc6c3bf075d1405a823943ed49a`
- **Coordinator integration branch**: `integration/workstation-r024-twenty`
- **Strategy**: 20 isolated substantial Rust workstream branches, one initial combined source commit, one full integration CI matrix, then one squash commit to `main` only after green required gates.
- **Test coverage**: 60 new focused host regression tests across 20 modules, two integrated crate tests, and x86-64 QEMU guest smoke.

## Implemented work

| Agent workstream | Feature | Real code scope |
|---|---|---|
| 01 | file-manager | Kernel-backed create/copy/move/delete/read with file/version validation and no collision overwrite |
| 02 | safe-trash | Actual file deletion with generation-safe RAM trash, restore and conflict prevention |
| 03 | bulk-rename | Atomic multi-file rename using validated snapshot and preimage file revisions |
| 04 | directory-size | Actual directory file-count, bytes, Rust/doc extension totals and free-slot reporting |
| 05 | storage-quota | Quota-enforced real file creation/overwrite, utilization accounting and fail-atomic capacity rejection |
| 06 | source-diff | Actual source-file byte diff, minimal changed span and bounded preview |
| 07 | source-symbol-search | Project-wide Rust symbol navigation based on real file contents and existing source outline |
| 08 | editor-selection | UTF-8 boundary-safe source selections, copy and exact stale-safe file replacement |
| 09 | editor-diagnostics | Compiler severity/line/column to real source byte offset, version checked navigation and filtering |
| 10 | editor-save-queue | Multi-document save-all with owner session identity and atomic real RamFs update |
| 11 | terminal-write | Working typed terminal file commands touch/write/append/remove and read, no arbitrary exec |
| 12 | terminal-paths | Terminal working path normalization, parent navigation, traversal rejection and actual-file resolution |
| 13 | terminal-history | Generation-isolated terminal command recall, search, privacy filtering and bounded ring |
| 14 | terminal-pipelines | Read-only cat/grep/head text pipelines on actual filesystem files without shell execution |
| 15 | project-config | Validated project key/value configuration editing and revision-safe source writes |
| 16 | project-locks | Authenticated, expiring source-file write locks bound to PID generation and file revision |
| 17 | workspace-launcher | Desktop/Developer launcher shortcuts for actual editor/project files with capability and revision checks |
| 18 | build-cache | Trusted build artifact metadata cache invalidated on real source revision/toolchain changes |
| 19 | test-coverage | External-runner-attested line-hit coverage on actual source file revisions and threshold gating |
| 20 | guest-integration | Cross-feature QEMU smoke: file manager/trash, editor, terminal, config, locks, launcher, cache and coverage |

## Integration and acceptance

1. Rust host test all new `no_std` kernel modules, adversarial access and stale revisions.
2. QEMU guest smoke under existing `test-r041-workspace`, with exact serial marker `IndianOS: R024 live file-manager-editor-terminal developer workstation PASSED`.
3. SHA, S0, Orbit GUI/QEMU, M1 durable VFS, M2 signed app/network acceptance and full kernel/QEMU pass at same integration head SHA.
4. Independent reviewer check passes. Coordinator merges PR via **squash** so 20 scoped workstreams appear as one commit on `main`. Never merge an incomplete or failing required build.

## Product boundary

The new implemented file management and editor features work on actual `RamFs` guest bytes but are **not persistent across reboot** unless integrated separately into M1 durable storage. Build caching and coverage require a trusted external build/test runner; this work does **not** ship a native compiler, arbitrary executable shell, complete IDE GUI, cryptographic OTA updater or third-party application compatibility.  twenty isolated branches does not imply twenty concurrently executing autonomous AI developer runtimes.
