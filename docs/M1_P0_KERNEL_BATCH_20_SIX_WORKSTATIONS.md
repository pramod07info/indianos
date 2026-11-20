# Kernel-first P0 batch: 20 owned implementation items, six workstations

Integration base main: `10300f5e52efc87044a294c1d1cdedeb438ae693`. PR work branch: `feat/p0-kernel-six-workstations-20-20261117`.
The original M1 90-task and R123 ledgers retain all full-acceptance
checkboxes until exact-head guest and hardware evidence qualifies them.

| ID | Workstation | Tested software change |
|---|---|---|
| P0-01 | W1 | Reject reclaim before all exact AP lifecycle acknowledgements |
| P0-02 | W1 | Reject stale or replayed AP receipt and duplicate AP fence |
| P0-03 | W1 | Protect live PID1 incarnation from pressure/teardown batch |
| P0-04 | W1 | Atomically evict only exact acknowledged scheduler incarnations |
| P0-05 | W2 | Track bounded high-priority dispatch streak in scheduler |
| P0-06 | W2 | Guarantee low-priority dispatch after bounded high quota |
| P0-07 | W2 | Reject invalid CPU and zero quantum without changing cursor |
| P0-08 | W2 | Keep exact PID/epoch runnable membership unchanged on failure |
| P0-09 | W3 | Authenticate sender PID and generation at multi-message ingress |
| P0-10 | W3 | Authenticate exact recipient and reject stale PID reuse |
| P0-11 | W3 | Stage bounded FIFO payloads before all-or-none publication |
| P0-12 | W4 | Bind authenticated VFS reads to exact COW generation |
| P0-13 | W4 | Reject cross-owner reads and failed second path without output leaks |
| P0-14 | W4 | Verify two-file byte recovery after cold block-device remount |
| P0-15 | W5 | Require explicit trusted NIC driver IRQ/DMA halt callback |
| P0-16 | W5 | Hold RX shutdown while in-flight software handlers remain |
| P0-17 | W5 | Reopen ingress only after checked drain and new ring epoch |
| P0-18 | W6 | Revalidate authenticated boot measurement before PID1 handoff |
| P0-19 | W6 | Revalidate sealed rollback floor and selected signed slot |
| P0-20 | W6 | Deny launch on stale receipt or changed boot identity |

## Real changes and limitations

- **W1**: `process_events.rs`: scheduler transaction consults all independent
  AP acquire/release acknowledgements, verifies current PID1 authority and
  stages exact-generation process removals atomically. **No APIC IPI or TLB
  invalidation** is implemented by this slice; the privileged caller must
  serialize scheduler mutations and prove physical TLB invalidation.
- **W2**: `process_events.rs`: bounded high-priority scheduling quantum with
  low-priority fairness and no state mutation on invalid input. **No live
  timer-ISR preemption or FPU switching** is implemented.
- **W3**: `ipc.rs`: exact-epoch sender/receiver authenticated ordered IPC
  multi-message admission, fully preflighted payload lengths and capacity;
  no partial publication. **No SMP IPC lock or new syscall ABI**.
- **W4**: `hier_vfs.rs`: real CRC-verified block-backed dual-file reads with
  credential checks and COW-generation pinning, staged caller output,
  and cold-remount tests. **No multiwriter SMP mount lock**.
- **W5**: `nic_rx_fence.rs`: call a trusted NIC-driver hardware shutdown
  boundary before bounded software completion drain and checked ring rebind.
  **The actual NIC driver must mask interrupts, disable bus mastering, drain
  descriptors, and prove DMA/IOMMU quiescence**; the test double is not hardware
  certification.
- **W6**: `r114_boot_epoch.rs`: independent full authenticated PID1 boot
  admission redo just before executable handoff. **No production firmware TPM
  implementation**, and the live launcher still must call this under a
  serialized boot-state lock.

### Required tests

Six dedicated GitHub Actions matrix workstations run the exact-head
`kernel_p0_w1` through `kernel_p0_w6` Rust regressions plus no_std
freestanding compilation. Existing mandatory kernel boot, actual QEMU
persistence, two-vCPU, security, integration and reliability workflows must
also pass on the new SHA. No suppressed/skip checks accepted.

**These six are CI verification workstations; no six independently running
coding agents have been launched.** A green CI build does not complete
physical multicore, NIC DMA, TPM, real user-facing desktop or full M1.
Require independent review and branch protection before merging.
