# IndianOS kernel-first P0 batch 17 — tasks 361–400

Target: existing draft [PR #786](https://github.com/pramod07info/indianos/pull/786); previous exact head `d17168c049c8fa6c4d7eb1fee42a46d3072ba072`. Forty distinct **source-level** obligations, across six GitHub Actions validation workstations. They are not six launched development agents or hardware-release acceptance gates.

| P0 ID | Workstation | Implementation/acceptance obligation |
|---|---|---|
| 361 | W1 — SMP/TLB | Authenticate a complete multi-AP dispatch topology before sending IPIs |
| 362 | W1 — SMP/TLB | Validate disjoint canonical page ranges and exact epoch |
| 363 | W1 — SMP/TLB | Enforce 4096-page aggregate invalidation budget |
| 364 | W1 — SMP/TLB | Reject empty, overlapping, out-of-order or stale plans |
| 365 | W1 — SMP/TLB | Send one immutable plan to each AP with one exact lifecycle receipt |
| 366 | W1 — SMP/TLB | Expose all published receipts when a later IPI send fails |
| 367 | W1 — SMP/TLB | Require every AP to invalidate all pages before reclaim acknowledgement |
| 368 | W2 — Process lifecycle | Limit forwarded lifecycle notices to exact Exited kind and incarnation |
| 369 | W2 — Process lifecycle | Protect PID1 from user-process exit forwarding |
| 370 | W2 — Process lifecycle | Authenticate every receipt against PID and process generation |
| 371 | W2 — Process lifecycle | Reject duplicate (PID,generation) pairs in one batch |
| 372 | W2 — Process lifecycle | Reject a generation already published to supervisor |
| 373 | W2 — Process lifecycle | Stage both source-dequeue and supervisor-enqueue atomically |
| 374 | W2 — Process lifecycle | Keep both event queues unchanged if final supervisor capacity fails |
| 375 | W3 — Authenticated IPC | Pin every observed source mailbox to exact receiver generation |
| 376 | W3 — Authenticated IPC | Pin sender identity and SHA-256 payload hash before delivery |
| 377 | W3 — Authenticated IPC | Enforce observed mailbox occupancy as a precondition |
| 378 | W3 — Authenticated IPC | Reject duplicate source mailbox identities preflight |
| 379 | W3 — Authenticated IPC | Reject short caller buffer before reading any inbox |
| 380 | W3 — Authenticated IPC | Enforce bounded aggregate byte quota across all reads |
| 381 | W3 — Authenticated IPC | Commit all mailbox dequeues only after entire batch verifies |
| 382 | W4 — Persistent metadata | Validate three distinct in-range replica sector addresses |
| 383 | W4 — Persistent metadata | Pin all six reads to one externally authenticated media revision |
| 384 | W4 — Persistent metadata | Independently reread each of three metadata replicas |
| 385 | W4 — Persistent metadata | Compute SHA-256 for each stable replica before acceptance |
| 386 | W4 — Persistent metadata | Require at least two replicas match the trusted manifest |
| 387 | W4 — Persistent metadata | Recover from exactly one corrupt replica without writes |
| 388 | W4 — Persistent metadata | Retire unstable/no-quorum view without changing caller data |
| 389 | W5 — QEMU NIC ingress | Reuse the same RX admission fence for actual ARP and ICMP QEMU stages |
| 390 | W5 — QEMU NIC ingress | Acquire a non-Send permit before each ICMP Ethernet descriptor poll |
| 391 | W5 — QEMU NIC ingress | Verify ICMP peer, checksum, fixed echo ID/sequence and payload |
| 392 | W5 — QEMU NIC ingress | Reject malformed reply within bounded polling budget |
| 393 | W5 — QEMU NIC ingress | Revoke in-flight echo result if NIC quiesces while polling |
| 394 | W5 — QEMU NIC ingress | Refuse foreign NIC/device identity without accepting a reply |
| 395 | W6 — PID1 launch | Generate deterministic domain-separated PID1 mapping transcript |
| 396 | W6 — PID1 launch | Bind executable file-content hashes and segment permissions to transcript |
| 397 | W6 — PID1 launch | Bind entry point, BSS, stack extent and validated ELF layout |
| 398 | W6 — PID1 launch | Derive expected PTE hash internally rather than trusting arbitrary caller input |
| 399 | W6 — PID1 launch | Require measured mapper audit to match derived digest before publication |
| 400 | W6 — PID1 launch | Abort malformed/forged mapping while preserving secure one-shot launch semantics |

## New source-level regressions
- W1 `smp_lifecycle_fence.rs`: `kernel_p0_batch17_w1`; multi-AP immutable plan, partial dispatch, AP acknowledgement, unsafe range denial
- W2 `process_events.rs`: `kernel_p0_batch17_w2`; exact incarnation exit batch, duplicate/stale and two-ring rollback
- W3 `ipc.rs`: `kernel_p0_batch17_w3`; authenticated observed mailbox snapshot, digest, owner, quota and atomic output
- W4 `block.rs`: `kernel_p0_batch17_w4`; three-replica authenticated metadata quorum; one bad replica recovery and unchanged-output failure
- W5 `network.rs`: `kernel_p0_batch17_w5`; actual R028 ARP/ICMP shared-fence integration, polled descriptor before parser
- W6 `r114_boot_epoch.rs`: `kernel_p0_batch17_w6`; deterministic ELF-layout mapping digest and authenticated one-use launch

CI: six GitHub Actions matrix jobs each run its matching new filter plus P0-001–360 legacy regressions and freestanding x86_64 `no_std` build, all on the **same exact new commit SHA**. A successful source test is not evidence of production hardware. A digest-only IPC snapshot without an authenticated monotonic per-message receipt is not replay-proof against a later identical message.

## Still OPEN
Physical APIC IPIs, real INVLPG ordering, multicore locks, PCI IRQ/DMA/IOMMU fencing, production UEFI/TPM and ring-3 PTE readback, independent review, sudden power-loss recovery, and exact-head end-to-end QEMU desktop/release acceptance. Keep PR draft pending independent protected merge requirements.
