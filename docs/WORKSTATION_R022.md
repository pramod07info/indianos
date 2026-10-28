# IndianOS R022 — Twenty large developer-workstation source-workflows

**Baseline main**: `38ee818ced36da4a4a82b49820b1f970e835f7e6`  
**Coordinator integration**: `integration/workstation-r022-twenty`  
**Delivery**: 20 separately committed functional Git branches, 20 no_std-compatible Rust modules, each with three unit/adversarial tests, merged as a single initial integration commit and one combined CI run.

## Implemented engineering scope

| Workstream | Feature area | Actual code functionality |
|---|---|---|
| 01 | workspace-tree | Live directory enumeration, sorted tree, file size totals and extension counts from actual RamFs |
| 02 | directory-relocation | Atomic all-files directory relocate with snapshot rollback and path/collision validation |
| 03 | editor-save-sync | Dirty editor save/refresh/close against actual file revisions with stale overwrite prevention |
| 04 | cross-file-search | Cross-file literal search of live project bytes with path, offset and source line |
| 05 | file-revisions | Capture and restore prior actual file data with stale revision guard |
| 06 | project-integrity | File digest inventory, version checks and fail-atomic project integrity verification |
| 07 | source-bookmarks | Revision-checked editor bookmarks, retargeting and owner-gated source navigation |
| 08 | editor-multi-edit | Atomic multi-range source replacement with nonoverlap, exact preimage and capacity guards |
| 09 | project-export-index | Actual source export manifest with content checksums, revision and byte totals |
| 10 | project-restore | Owner-generation staged snapshot restoration with checksum validation and dry run |
| 11 | build-target-matrix | Desktop/Developer/Server build matrix with dependency graph and ABI/toolchain checks |
| 12 | rust-test-discovery | Discover Rust #[test] source declarations directly from RamFs with lines and names |
| 13 | diagnostic-summary | Owner-isolated compiler diagnostic inbox, parser, dedupe and source severity totals |
| 14 | workspace-session-snapshot | Version-bound open-file tabs and focused editor session restoration |
| 15 | command-execution-gate | Capability-gated typed compile/test/lint/package actions with trusted execution evidence |
| 16 | terminal-process-lifecycle | Foreground/background terminal job lifecycle, stop/resume, exit/reap and owner checks |
| 17 | capability-lease | Expiring, generation-scoped and use-limited rights leases with revocation |
| 18 | resource-governor | Desktop-interactivity memory reserves, CPU quota admission, shrink and reclaim |
| 19 | release-artifact-check | Release gating based on real file manifest, all green builds, tests and independent approval |
| 20 | end-to-end-acceptance | Guest-safe source-edit-search-test-build-release-restore acceptance across multiple modules |

## Regression and integration tests

- 60 module-focused Rust tests plus two crate integration smoke/security tests.
- Real RamFs file editing, tree browsing, multi-file search, checksum integrity, snapshot dry-run import, build matrix and release gating tested together under `agent_20::smoke()`.
- QEMU feature `test-r041-workspace` additionally asserts `IndianOS: R022 developer workstation source-edit-build-release PASSED` and emits a fatal marker on mismatch.
- Requires exact-head Roadmap SHA, S0 security, Orbit UI, M1 VFS/QEMU persistence, M2 signed native app/QEMU network, full kernel boot/QEMU run, and independent reviewer.
- Do not auto-merge while any required check remains in progress or fails. Final merge should **squash** to one mainline commit.

## Explicit production boundaries

Real source file operations here use `RamFs` and are therefore **not durable across reboot**; durable VFS remains an independent subsystem. The terminal/compile/test execution APIs implement trusted policy and result tracking but do not directly launch processes, invoke a native Rust compiler, or perform an actual GitHub merge. No complete GUI IDE, secure network update installer, hardware GPU/QPU execution, or external binary compatibility is claimed.

The 20 Git branches denote **20 scoped development workstreams** implemented via the repository connector, not 20 independent agent runtimes.
