# M1 normal startup and ownership integration

Base: `442cabc9001f868dfdcabe7bcf485752975dece9`. The exact base's kernel,
M1 persistent desktop, combined regression, M2 and S0 GitHub workflows passed.
No open PR existed at selection. R051–R054 changes were inspected and retained.

Six independent coding workers used isolated Git worktrees. Maximum concurrent
execution was six workers and the coordinator, not twenty workers. Remaining
workstreams are queued behind M1 dependencies and this batch's integration gate.

## Acceptance ledger

The five checked foundations in DELIVERY_MILESTONES.md remain accepted at their
existing limited scope: Rust/UEFI boot, ring-3/syscall/IPC experiments, opt-in
three-process graphical startup, bounded persistent Files/VFS, RAM command store.
This batch does not expand those claims to production or physical hardware.

| Gate | Required outcome and previous state | Changes in this batch | Final status |
|---|---|---|---|
| G1 | Live authenticated durable Terminal read/write/mkdir/mv and reboot GUI readback; open | Bounded live directory listing and persisted-note length supplement existing write/cat | IN PROGRESS: fixed paths and no live mkdir/mv |
| G2 | Validated selected partitions, corruption/write-failure and QEMU remount; open | Capacity-pinned discovery and final GUID validation during selected mount | IN PROGRESS: selected-GPT guest acceptance outstanding |
| G3 | Normal PID1, isolated compositor, Files/Terminal and recovery; open | Normal ring-3 bootstrap console rather than staging-only halt | IN PROGRESS: graphical normal startup and service supervision outstanding |
| G4 | Independent ring-3 compositor/input services and guarded surfaces; open | Queued damage binds to grant and surface incarnations | IN PROGRESS: primitive hardening, window management remains privileged |
| G5 | Physical keyboard/pointer, focus and visible app actions; open | Physical QEMU Terminal commands exercise existing input path | IN PROGRESS: combined normal/IRQ/service qualification outstanding |
| G6 | Durable VFS, flush/fsync, locking/isolation and power cuts; open | COW truncate/zero extension and cut regressions | IN PROGRESS: general locking, ACLs and hardware flush qualification outstanding |
| G7 | Independent live syscall CRUD/rename with reboot and ownership; open | Descriptor truncate and scheduler abort authority retirement | IN PROGRESS: complete cross-process operation set outstanding |
| G8 | Supported emulator/physical drivers, shutdown/restart/recovery; open | Normal PID1 termination/recovery and retry-safe page-table teardown | IN PROGRESS: physical qualification and production restart outstanding |

No new M1 or M2 gate is accepted. S0 regression success does not complete S0–S4:
universal filesystem mediation, hardware trust roots, independent security audit
and administrator-enforced protection remain separate release requirements.
Additional verified completion is not measurable from available evidence.
The requested approximate 12% overall / 40% experimental-kernel estimates remain
unchanged and have low confidence; they are not acceptance measurements.

## Integration and evidence

Subsystem documentation records exact contracts and tests. The coordinator
reviews worker diffs, resolves shared runtime/userland changes, runs combined
host/no_std/security/M2 regressions and actual disposable-disk QEMU tests, and
records exact commit/check URLs in the PR. A green compilation alone cannot
close any gate above. No physical disks are opened or formatted by this batch.

Local combined host tests passed 2,266 cases, Python acceptance controls passed
111, Terminal parser tests passed six and bootstrap parser tests passed four.
Combined freestanding compilation passed with existing warnings retained.
The M2 host subset passed 152 tests. Real QEMU passed normal PID1 and four
damaged-init cases, two-boot physical Terminal write/cat/ls/stat, five-boot VFS
with native truncate/corruption recovery, IPC, APIC preemption and supervisor
crash/restart. Existing graphical Files reboot acceptance also passed.

Integration caught and corrected the action-6 syscall whitelist, missing OVMF
path forwarding and an incremental Limine extraction/build timestamp defect.
No tests were disabled. Retry-safe published page-table release is covered;
simultaneous clone-construction and release-backend failure remains a documented
best-effort cleanup limitation. Hardware, SMP and universal ACLs remain open.

## Next ten functional priorities

1. Connect the live Terminal broker to an explicitly selected GPT session.
2. Add authenticated live mkdir/rename/create/delete with GUI reboot readback.
3. Start dependency-ordered desktop services through normal PID1.
4. Move window management into an independent guarded ring-3 compositor.
5. Separate the authenticated input broker and qualify IRQ delivery with IPC.
6. Add live selected-GPT corrupt-media and failed-write QEMU acceptance.
7. Define serialized volume ownership, flush/fsync and shutdown ordering.
8. Inject QEMU power cuts during real ATA data and metadata publication.
9. Add bounded process cancellation, peer-death notification and supervision.
10. Persist ownership/permissions and enforce them across every mutation route.
