# M1 R052 — Five substantive subsystem workstreams, one integration commit

IndianOS M1 completion-first gate tracker [#616](https://github.com/pramod07info/indianos/issues/616) remains authoritative. These are five production deliverables, **not** five independently running Codex sessions and not five closed M1 acceptance gates.

| Functional workstream | Changed production behavior | Regression acceptance |
| --- | --- | --- |
| 1. IPC message delivery | Transactional, receiver-authorized bounded batch receive, private FIFO copy-before-dequeue, retains all queued messages if a caller's sink fails | Wrapped FIFO, sender identity, failure atomicity, invalid sizes and peer isolation |
| 2. GUI compositor queue | Trusted compositor purges damage for revoked window grants or invalid surface boundaries before fair presentation | Window closure, preservation of peer frames, short output cannot consume valid presentation |
| 3. PID1 service supervisor | Atomic multi-service crash/replacement plan with deferred preparation, no partial service restart or action output on an invalid/exhausted request | Multi-service recovery, wrong generation rejection, duplicate/exhausted rollback |
| 4. Runtime persistent file handles | An ambiguous disk write / invalid volume retires its owner handle without falsely advancing cursor, mutation budget or durable-success count | Injected BlockDevice I/O failure, disk remount, retained old bytes, re-open requirement and benign invalid seek retry |
| 5. GPT workstation/Desktop selection | Double-check all GPT identity metadata after COW health probe; fail closed if removable media changes GUID during health reads even if LBAs unchanged | Healthy read-only baseline and fully checksummed mid-preflight identity swap |

Implementation reuses existing no_std kernel storage, GUI, IPC and runtime modules. Ten focused `m1_r052_*` Rust tests accompany the production changes. No new external dependencies, disk formats, superficial agent slot files or physical disk writes.

## Supported and unsupported behavior
The changes strengthen kernel primitives; they do not implement a fully isolated userland compositor, unlimited POSIX file descriptors, SMP block-device serialization, unattended physical installation, or general production PID1. Compositor validation authenticates current owner/window IDs, but cannot resolve identical numeric IDs recycled between checks until window grant epoch is included in the damage record. The generation-bound file handle policy from R051 remains conservative: unrelated filesystem mutations also require handle reopen.

All storage tests run on disposable simulated or QEMU disks. An explicit session must continue to validate rights and media. A GPU, quantum feature, full hybrid executable compatibility and the M2 workstation edition are outside the current M1 acceptance scope.

## Verification and merge requirements
Use the exact PR head SHA for Rust host/no_std, IPC/GUI/supervisor/file-handle/GPT tests, M1 persistent VFS and QEMU reboot, live terminal persistence, S0 security, runtime QEMU and full x86-64 kernel boot. Require branch protection and independent review. Do not mark M1 G2/G3/G4/G6/G7 checked merely because this PR builds.

Five major workstreams were requested. The current connected GitHub API does not launch isolated Codex coding agents; **actual independent coding agents started here: 0**. The integration changes are coordinator authored and checked as one commit.