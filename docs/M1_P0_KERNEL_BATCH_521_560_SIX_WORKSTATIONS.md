# IndianOS P0 batch 21 — 40 kernel-first tasks (521–560)

Draft PR: https://github.com/pramod07info/indianos/pull/786

**Implementation scope:** This commit changes six existing kernel production paths and adds regression tests. Six of the 40 tasks have PARTIAL source implementation, 34 are OPEN, and zero are fully acceptance-qualified. This document does not claim that six autonomous coding agents ran or that physical hardware trust, SMP, DMA or recovery is complete.

## Workstation ledger and acceptance
## W1: Physical memory and reclaim correctness (P0-521–527)

Depends on P0-481–487; current BSP-only PMM.

| ID | Implementation obligation | Acceptance evidence | Status |
|---|---|---|---|
| P0-521 | Fail closed on corrupt live PMM free-frame accounting | Use checked subtraction in actual exported free count; reject impossible retirement | PARTIAL — implementation + host tests |
| P0-522 | Tag every allocated physical frame with owning generation | Recycled PID or VMM owner cannot release another incarnation | OPEN |
| P0-523 | Reject duplicate retire tickets before clearing frame bitmap | Repeated reclaim cannot touch a newly reused frame | OPEN |
| P0-524 | Synchronize kernel allocator ownership across online APs | Two-vCPU concurrent allocate/free without aliasing or data race | OPEN |
| P0-525 | Acknowledge every target CPU TLB invalidate before freeing mapped frame | Real IPI, INVLPG and timeout recovery evidence | OPEN |
| P0-526 | Prohibit releasing DMA-pinned pages before device fences | No PMM release while NIC or storage device can write descriptors | OPEN |
| P0-527 | Demonstrate allocation exhaustion and recovery under four-vCPU QEMU | No fabricated free counts, double frees or reservation leaks | OPEN |

## W2: APIC/PIC timer and scheduler progress (P0-528–534)

Depends on P0-488–494; session epoch and monotonic tick.

| ID | Implementation obligation | Acceptance evidence | Status |
|---|---|---|---|
| P0-528 | Check source epoch on actual IRQ tick-publication path | Stale PIC/APIC ISR receipt cannot authorize a restarted timer session | PARTIAL — implementation + host tests |
| P0-529 | Retire in-flight IRQ callbacks before timer source switch | No old callback updates after new epoch publication | OPEN |
| P0-530 | Calibrate cross-CPU monotonic ordering under SMP | Observed scheduler clock never goes backward across APs | OPEN |
| P0-531 | Enforce generation-safe sleep deadline cancellation on process exit | Timed-out or restarted child cannot inherit old wakeup | OPEN |
| P0-532 | Add bounded preemption budget accounting to run queues | CPU-bound process cannot suppress unrelated runnable tasks | OPEN |
| P0-533 | Treat overflowed tick count as a permanent unsafe clock | Leases and timeouts deny authorization at saturation | OPEN |
| P0-534 | Verify 2/4-vCPU scheduler progress, fairness and signal races | QEMU execution traces establish latency and no stuck tasks | OPEN |

## W3: User-page privilege, system call and isolation (P0-535–541)

Depends on P0-495–501; ring-3 ABI and ProgramPlan.

| ID | Implementation obligation | Acceptance evidence | Status |
|---|---|---|---|
| P0-535 | Reject malformed stack bounds and executable+writable syscall destinations | Actual user_range syscall admission rejects malformed plans and W+X writes | PARTIAL — implementation + host tests |
| P0-536 | Prove user buffer permissions against live PTE flags before copy | Mapped user pages only; no kernel or unmapped address dereference | OPEN |
| P0-537 | Make scatter user copy atomic under a later-page fault | No partial destination publish across page-table replacement | OPEN |
| P0-538 | Enforce authenticated owner-generation for each syscall capability | Recycle PID, close/reopen and stolen descriptors denied | OPEN |
| P0-539 | Version syscall ABI and validate all registers and error channels | Ring-3 adversarial ABI clients cannot trigger supervisor confusion | OPEN |
| P0-540 | Protect trap restore against invalid selectors, flags and stack guards | Test actual IRETQ and nested fault path in QEMU | OPEN |
| P0-541 | Fuzz malicious ring-3 memory and IPC request combinations | Bounded allocations, no privilege escalation, serial traces | OPEN |

## W4: Storage identity and crash-consistent writes (P0-542–548)

Depends on P0-502–508; BlockDevice and mounted windows.

| ID | Implementation obligation | Acceptance evidence | Status |
|---|---|---|---|
| P0-542 | Propagate driver media-generation token into bounded block views | Same-capacity replacement invalidates active ReadOnlyDevice and BlockWindow | PARTIAL — implementation + host tests |
| P0-543 | Connect real ATA/NVMe/virtio driver identity changes to generation token | Hot-plug/reset increments epoch, never reauthorizes stale mount | OPEN |
| P0-544 | Bind mounted VFS to partition GUID and disk generation jointly | Wrong partition or recycled disk denied, even if capacity matches | OPEN |
| P0-545 | Write and sync data/metadata journal phases against actual device cache | Error never acknowledges uncertain durable state | OPEN |
| P0-546 | Replay journal deterministically after torn write/power interruption | Previous valid or last committed state, no ghost entries | OPEN |
| P0-547 | Make nested directory rename atomic across metadata and data barriers | No cycles/orphaned inodes under every crash injection point | OPEN |
| P0-548 | Verify graphical Files hashes across multiple QEMU cold boots | Exact-head disk, serial and guest files evidence | OPEN |

## W5: Virtio RX, offload and DMA ownership (P0-549–554)

Depends on P0-509–514; legacy NIC no offload negotiation.

| ID | Implementation obligation | Acceptance evidence | Status |
|---|---|---|---|
| P0-549 | Reject undersized Ethernet receive and unsupported virtio offload flags | Actual VirtioNet::poll returns error without publishing caller bytes | PARTIAL — implementation + host tests |
| P0-550 | Revalidate RX ring generation before rearming descriptor after reset | Old DMA used-ring entry cannot become new socket payload | OPEN |
| P0-551 | Mask, drain and acknowledge live NIC IRQ state before ring release | No device-owned DMA buffers recycled while IRQ pending | OPEN |
| P0-552 | Implement owner-generation-safe socket receive fencing in NIC driver | No cross-PID UDP delivery or stale reply from prior NIC epoch | OPEN |
| P0-553 | Qualify dual NIC and link flap with real QEMU traffic and PCAP | All send/receive ownership boundaries survive reset | OPEN |
| P0-554 | Verify physical DMA/IOMMU mapping ownership and completion isolation | Hardware-backed evidence, not only software mocks | OPEN |

## W6: Trusted boot handoff and desktop release proof (P0-555–560)

Depends on P0-515–520; Limine and initramfs.

| ID | Implementation obligation | Acceptance evidence | Status |
|---|---|---|---|
| P0-555 | Ensure boot initramfs returned file spans remain inside validated module | Actual boot_initramfs_file checks arithmetic and pointer containment | PARTIAL — implementation + host tests |
| P0-556 | Verify signed kernel and recovery slots against immutable trust root | Tampered boot/recovery images never execute | OPEN |
| P0-557 | Pin PID1 verified image identity and PTE permissions to one boot epoch | Ring-3 entry only after independent page-table readback | OPEN |
| P0-558 | Start terminal and Files services through actual PID1 dependency graph | Usable desktop and keyboard routing in normal QEMU boot | OPEN |
| P0-559 | Validate desktop storage persistence after forced reboot and recovery | Successful second boot preserves checksums and access controls | OPEN |
| P0-560 | Require full exact-SHA CI, protected review and final release evidence | No skipped gate; independent review before merge to main | OPEN |

## Exact integration scope

| CI workstation | Actual Rust production file | New test |
|---|---|---|
| W1 | `pmm.rs`: overflow-proof free-frame reporting and invalid frame retirement rejection | `kernel_p0_batch21_w1` |
| W2 | `timer.rs`: verify live timer epoch/source when an IRQ marks monotonic progress | `kernel_p0_batch21_w2` |
| W3 | `syscall.rs`: stack bounds and non-writable executable pages in user copy validation | `kernel_p0_batch21_w3` |
| W4 | `block.rs`: opt-in media-generation provider checked through real bounded views | `kernel_p0_batch21_w4` |
| W5 | `virtio_net.rs`: reject short Ethernet frames or nonzero unnegotiated RX offload header | `kernel_p0_batch21_w5` |
| W6 | `boot.rs`: verify every returned initramfs file pointer remains inside its module | `kernel_p0_batch21_w6` |

Each dedicated GitHub Actions lane must prove at least one nonempty real Rust test with its filter, compile for `x86_64-unknown-none` with `no_std`, and run against the **exact integration commit SHA**. Inherited workstations must also pass.

**Unqualified and open:** cross-core allocator/IPl shootdown, real syscall PTE user-copy, journal power-loss, NIC DMA/IOMMU and ring ownership, production TPM/UEFI signed boot, normal-boot usable desktop, destructive storage recovery, independent protected review.

**Do not merge the draft PR** based solely on the six host test gates. When recording completion, link the implementation line, negative tests, QEMU or physical evidence as appropriate, and the exact successful CI SHA.
