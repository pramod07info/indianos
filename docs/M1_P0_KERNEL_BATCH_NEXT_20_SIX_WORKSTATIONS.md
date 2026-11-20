# Kernel P0 second implementation batch: 20 additional priority tasks (21–40)

Based on existing open PR #786 at parent SHA `45aa98b67a41d10266843be9744c11f8736be82b`. This is a **second
distinct** set of twenty source-focused tasks, not another claim of twenty
completed hardware/milestone acceptance obligations.

| Task | Verification workstation | Source implementation / negative acceptance |
|---|---|---|
| P0-21 | W1 | Reject invalid source or destination CPU during batch migration |
| P0-22 | W1 | Pin every migration to exact PID, epoch and priority |
| P0-23 | W1 | Enforce explicit destination CPU affinity and protect PID1 |
| P0-24 | W1 | Roll back all migrations if a later target is full or invalid |
| P0-25 | W2 | Reject ring3 entry into NX/non-executable segments |
| P0-26 | W2 | Reject writable executable W^X pages |
| P0-27 | W2 | Reject overlapping mappings and malformed ELF spans |
| P0-28 | W2 | Revalidate guard and stack geometry and privilege flags |
| P0-29 | W3 | Retire multiple exact IPC process incarnations atomically |
| P0-30 | W3 | Remove queued messages from all retiring senders |
| P0-31 | W3 | Reject stale or mixed-sender FIFO delivery |
| P0-32 | W3 | Keep queue intact after failed destination copy |
| P0-33 | W4 | Read bounded metadata from two independent disk passes |
| P0-34 | W4 | Reject mismatched or torn reads without exposing output |
| P0-35 | W4 | Reject short/beyond-capacity and late I/O faults atomically |
| P0-36 | W5 | Bind parsed packet result to a live RX permit and ring revision |
| P0-37 | W5 | Reject NIC quiescence race and release permit on decoder error |
| P0-38 | W6 | Validate trusted boot chain twice before issuing launch receipt |
| P0-39 | W6 | Prevent duplicate in-memory PID1 launch from one boot epoch |
| P0-40 | W6 | Preserve launch gate after failed untrusted/changed-identity admission |

## Changes and critical trust boundaries

- **W1** `process_events.rs`: all-or-nothing, identity-exact cross-core
  runqueue migration with PID1 protection, affinity policy, capacity and
  priority checks. Requires actual scheduler locking, CPU interrupt and
  physical TLB invalidation in production.
- **W2** `syscall.rs`: stricter numeric ring3 page/span, NX, W^X, guard,
  stack and trap-frame validation. This NEW validator is NOT YET WIRED
  to the live trap/ELF loader. No claim of hardware NX policy closure.
- **W3** `ipc.rs`: one atomic exact-generation IPC retirement batch
  including queued sender messages, and sender-pinned FIFO receive prefix.
  Still requires live SMP synchronization and syscall copy wiring.
- **W4** `block.rs`: two complete bounded read observations before
  publishing metadata, refusing changed bytes, geometry, capacity faults
  and late I/O. Not disk authentication or actual power-cut journaling.
- **W5** `nic_rx_fence.rs`: entire software packet parsing stays within
  a live RX fence/epoch permit. Parser closure MUST have no external
  effects. Physical IRQ masking, bus-master disable and DMA drain remain
  unqualified.
- **W6** `r114_boot_epoch.rs`: in-memory single-use verified PID1 boot
  gate, with full refreshed trust-root, slot and journal checks before
  accepting a receipt. It cannot replace protected hardware monotonic
  counters or production TPM/UEFI integration.

Six GitHub Actions verification jobs now require `kernel_p0_w1..w6` plus
`kernel_p0_next_w1..w6`, alongside freestanding no_std compile on the
**new exact PR SHA**. The jobs are CI workstations, NOT independent coding
agents. Full original M1 and R123 hardware and release acceptance remains
OPEN; do not use the 40-item task count as a kernel completion percentage.
