# IndianOS P0 batch 14 — tasks 281–300

Target: existing draft PR [#786](https://github.com/pramod07info/indianos/pull/786), based on the exact P0-280 branch head `d05de066ed0f865625d09644734564b4cfcb723f`. This batch is **20 specific source-level changes**, not 20 hardware acceptance gates, and these six workstations are **six CI verification jobs**, not independently launched coding agents.

## Task-to-source acceptance ledger

| P0 ID | WS | Implemented contract | Required test evidence |
|---|---|---|---|
| 281 | W1 SMP | Derive canonical 4KiB page range from an unaligned user byte span | Two-page boundary regression |
| 282 | W1 SMP | Detect zero, overflow, user-limit crossing and overlong shootdowns | Invalid-range cases reject |
| 283 | W1 SMP | Require requested byte span to exactly match pinned shootdown range/epoch | Mismatch calls no invalidator |
| 284 | W1 SMP | Acknowledge pending AP only after all exact pages are flushed | Exact callback sequence and AP receipt |
| 285 | W2 Process | Transactionally acknowledge a bounded lifecycle event prefix | Two events removed atomically |
| 286 | W2 Process | Prevent sequence-reused, out-of-order or duplicate head consumption | Stale-later-prefix rollback |
| 287 | W2 Process | Preserve survivor event FIFO after exact batch consumption | Next receipt unchanged |
| 288 | W3 IPC | Pin expected sender identity for every input mailbox | Wrong sender rejected |
| 289 | W3 IPC | Reject duplicate source mailbox generations in multi-hop relay | Duplicate rejected before commit |
| 290 | W3 IPC | Enforce checked aggregate relay byte quota | Quota failure preserves source FIFO |
| 291 | W3 IPC | Atomically transfer heads to authenticated destination as relay identity | Later destination-full rollback and FIFO |
| 292 | W4 Storage | Compare independent physically distinct redundant sector LBAs | Four verified underlying observations |
| 293 | W4 Storage | Fence both reads to one externally pinned media revision | Changed revision permanently retires view |
| 294 | W4 Storage | Publish bytes and SHA-256 receipt only after complete matching copies | Changed copy never changes output |
| 295 | W5 Network | Validate bounded Ethernet/IPv4/ICMP echo reply checksums | Valid padded echo accepted |
| 296 | W5 Network | Require trusted peer MAC/IP and exact echo ID/sequence, deny fragments/injected padding | Spoof and malformed cases rejected |
| 297 | W5 Network | Tie parser outcome to NIC RX-generation admission | Quiescing during parsing revokes |
| 298 | W6 Boot | Introduce explicit non-default PID1 staged-PTE audit interface | Trusted mapper callback invoked |
| 299 | W6 Boot | Audit unpublished mappings before one-shot PID1 entry publication | Audit failure blocks publication |
| 300 | W6 Boot | Abort failed audit and permanently consume boot admission | Repair cannot replay same epoch |

## Validation

The exact-head GitHub Actions workflow `.github/workflows/kernel-p0-six-workstations.yml` runs `kernel_p0_batch14_w1` through `kernel_p0_batch14_w6` on the same workstation profiles as previous P0 batches, **all earlier P0 filters**, exact-head checkout validation and freestanding `x86_64-unknown-none` `no_std` compilation. These new checks are **PENDING until their GitHub Actions run completes on the resulting commit**.

Mandatory independently verified requirements remain **OPEN**: actual multicore APIC IPI delivery and INVLPG/TLB ordering, hardware NIC IRQ/DMA/IOMMU, authenticated UEFI/TPM, live ring-3 page-table mapping, physical disk power-cut recovery, kernel/QEMU boot and persistent desktop, final reliability/security/release gates, protected branch and independent review. No merge or 100% acceptance declaration on the strength of host mocks alone.
