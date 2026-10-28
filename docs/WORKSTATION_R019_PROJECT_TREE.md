# R019 Developer Workstation — first real project-tree integration

## Workstation integration commit
Starting main: `5feeef726fb0d8246626d29efb91ece0c6d2974c`
Branch: `integration/workstation-r019-project-tree`

This is **one cohesive source integration increment**, not 40 agents. It implements a real bounded disk-backed project tree with authenticated desktop service calls. The existing `HierVfs` runs on R027 copy-on-write sectors and currently supports at most eight stored objects. No graphical file tree, native compiler or actual swarm of 40 coding processes is claimed.

## Production behaviors introduced

- Persistent `/work` project root and project-directory create.
- Attach an existing directory and list its committed children.
- Create, read, overwrite, rename and delete project files within validated leaf names.
- Rename an entire project directory via the existing atomic tree rename implementation.
- Remove only empty project directories; never recursively destroy source.
- Explicit refresh after any external volume-generation change; reject stale writes.
- Exact process PID/generation session checks; Desktop separately requires FILE_READ / FILE_WRITE capabilities for each operation.
- Strict leaf-name grammar and path-length caps prevent `..` and separator escape.
- Host unit tests exercise create → write → rename → full disk remount, stale generation, wrong PID incarnation, invalid paths, nonempty deletion refusal, and insufficient output buffers.
- Desktop integration tests exercise owner access, rejected recycled PID, actual on-disk bytes and expiry of capabilities.

## File changes
1. `rust/kernel_core/src/m1_project_workspace.rs`: real project API with 6 regression tests.
2. `rust/kernel_core/src/m1_persistent_desktop.rs`: 12 authenticated project service operations plus 2 cross-layer tests.
3. `rust/kernel_core/src/lib.rs`: register bounded module only when existing test-r031-vfs feature is enabled.
4. This acceptance/status ledger.

## Unified CI
The one integration PR will automatically trigger the existing full kernel/QEMU job and security, SHA, M1/M2 acceptance checks for its exact SHA. Never count a job as passed before its conclusion is success. This workflow is a single **source integration** with one full kernel build per candidate, not 40 separate kernel builds. If fixes change the SHA, rerun the complete gate.

## Remaining major tasks
Tracked in issue #544. WS01 and WS03 have gained **bounded service functionality**, not full end-to-end acceptance. Work to wire actual ring-3 syscall ingress, on-screen project file explorer and a native IDE remains. The remaining 38+ tasks (and unverified portions of WS01/WS03) are not complete.

**Actual independent coding agents started by this commit: 0.** This work was made through the connected GitHub editing session. Branches and assigned task labels are not autonomous AI workers.
