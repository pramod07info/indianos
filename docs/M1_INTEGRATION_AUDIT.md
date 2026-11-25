# M1 integration evidence

Baseline main: `b637ce5df22df5c9e05b5d55f0d0442bcf312bbf`. Its
production parent chain includes #812 and #813. The two subsequent commits
archive screenshots. Issue [#616](https://github.com/pramod07info/indianos/issues/616),
the [milestone checklist](DELIVERY_MILESTONES.md), [task registry](M1_TASK_REGISTRY.json)
and [phase scope](M1_PHASE_SCOPE.md) govern acceptance.

- Formal M1 acceptance: **5/13 (38.46%)**.
- Detailed mandatory phase-task audit: **0/99 fully verified**. All 99 registry
  rows still require audit. This is an evidence count and carries no estimate
  of how much code exists. Treat the five accepted checklist positions and
  the 99 phase tasks as separate measures.

## Gate decisions at intake

Positions 1–5 are the original accepted foundations. Issue #616 assigns G1
through G8 to positions 6–13.

| Position | Production path under `rust/kernel_core/src` unless specified | Existing evidence | Remaining qualification or implementation |
| --- | --- | --- | --- |
| 1 · Rust boot | `boot.rs`, `kernel_main.rs`, `Makefile` | Accepted foundation, UEFI and kernel entry. | Preserve normal boot regression. Physical qualification is tracked below. |
| 2 · Ring 3 | `user_runtime.rs`, `user_image.rs`, `syscall.rs`, `ipc.rs` | Accepted process and IPC foundation. | Preserve isolation and authenticated IPC checks. |
| 3 · Limited GUI | `gui.rs`, `user_runtime.rs`, `userland/rust/src/main.rs` | Accepted opt-in R031.1 desktop. | Preserve legacy GUI boot. Normal startup and composition are G3/G4. |
| 4 · Bounded VFS | `hier_vfs.rs`, `durablefs.rs` | Accepted disposable ATA R031.9 Files path. | Preserve reboot and data readback. Production durability is G6. |
| 5 · RAM file store | `batch7_terminal.rs`, `batch7_ramfs.rs` | Accepted RAM interpreter/file-store foundation. | Preserve the storage-independent interpreter. Selected-disk behavior is G1. |
| 6 · G1 | `m1_persistent_terminal.rs`, `m1_persistent_desktop.rs`, `hier_vfs.rs` | #795 reconciled #790 terminal, selected GPT and cold-reboot graphical readback. | Refresh exact-revision live read/write/mkdir/rename evidence and resolve formal sign-off. No production gap identified in this intake. |
| 7 · G2 | `m1_partition.rs`, `block.rs`, `ata_pio.rs` | #795 reconciled #790 defensive partition and bounded ATA retirement evidence. | Refresh negative MBR/GPT, mirror/CRC, duplicate IDs, unchanged-media recovery and neighboring-sector checks. Resolve formal sign-off. Selected hardware is tracked in G8. |
| 8 · G3 | `kernel_main.rs`, `user_runtime.rs`, `userland/rust/src/desktop_startup.rs` | #806 improved live-map frame queries. #813 candidate passed actual normal startup, premature exits for all four roles and unready compositor recovery. | Establish shutdown exhaustion evidence at the required level and disposition the historical timeout. Complete the acceptance decision. |
| 9 · G4 | `gui.rs::paint`, `app_pixels.rs`, `userland/rust/src/main.rs::compose_desktop_strip` | #797 guarded app raster, #812 mapped canvases, #813 generation-bound readback and private region composition. | Implement full-frame ring-3 composition/presentation and PID4-only recovery with the same live clients and fresh authority. Require exact graphical, isolation and recovery evidence. |
| 10 · G5 | `m1_input.rs`, `ps2_mouse.rs`, IRQ delivery and `tools/test_normal_desktop_qemu.py` | Normal IRQ ingress and physical-input QEMU oracle in the current matrix. | Refresh keyboard/pointer/focus/capture, screenshots and transport-loss evidence. Audit pending P9 input criteria. |
| 11 · G6 | `durablefs.rs`, `hier_vfs.rs`, `fs_handles.rs` | Selected volume identity, read leases, locks and guest cuts inside publication exist. | Audit remaining concurrent-writer and per-user requirements. Record exact recovery receipts and physical flush/cache-loss qualification where required. |
| 12 · G7 | `fs_handles.rs`, `m1_persistent_desktop.rs`, `user_runtime.rs` | Authenticated descriptor operations, leases and process-specific denials exist. | Audit independent process create/read/edit authority, denied operations, and exact persisted data after same-disk reboot. |
| 13 · G8 | `ata_pio.rs`, `block.rs`, `user_runtime.rs` | ATA IDENTIFY validation, bounded media retirement and emulator recovery exist. | Qualify selected physical devices and remaining driver, shutdown/restart and negative security exits. Physical machine receipts are outstanding. |

## Revision-bound evidence and policy

- #808 merged a G1–G3 review packet. #810 records a scoped owner-authorized
  test-based acceptance proposal. The canonical checklist still requires
  sign-off and the proposal calls for reconciliation before activation.
  No gate decision is recorded in this integration intake.
- #813 candidate `248424f050492ee9a24d5c3fa6f454fec0b0dddb` passed
  [reliability](https://github.com/pramod07info/indianos/actions/runs/45601665265),
  [durable VFS](https://github.com/pramod07info/indianos/actions/runs/45601665587),
  [kernel boot](https://github.com/pramod07info/indianos/actions/runs/45601665287),
  [S0](https://github.com/pramod07info/indianos/actions/runs/45601665597),
  [duplicate prevention](https://github.com/pramod07info/indianos/actions/runs/45601665451)
  and [M2](https://github.com/pramod07info/indianos/actions/runs/45601665449).
  The [normal desktop job](https://github.com/pramod07info/indianos/actions/runs/45601665265/job/139161864031)
  reports the four premature-exit negatives and unready compositor recovery
  passed. [Artifact 15016533576](https://github.com/pramod07info/indianos/actions/runs/45601665265/artifacts/15016533576)
  is named for that candidate SHA. The production merge is `ec046484`,
  with separate [kernel boot evidence](https://github.com/pramod07info/indianos/actions/runs/45607495759).
  Record fresh candidate and merged-main reliability receipts for final acceptance.

### G3 shutdown exhaustion evidence

Candidate `bd74b98978ee452aacd6a023967750a584ef6b31` passed the new
[live QEMU shutdown exhaustion scenario](https://github.com/pramod07info/indianos/actions/runs/45616480940/job/139213083624).
The oracle requires two armed shutdown attempts, one restart, read-only
recovery and unchanged selected media.
[Artifact 15022933098](https://github.com/pramod07info/indianos/actions/runs/45616480940/artifacts/15022933098)
retains the exact-candidate evidence. Formal reconciliation and merged-main
qualification are still pending.
