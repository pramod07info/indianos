# IndianOS R122: 70 new development tasks + 30 inherited acceptance gates

**Base SHA:** `01f7755eab9a5059c58f11267df5f45eb00d41ae` (R121 merge #758). **Five logical workstreams**, **one source integration commit**, **one PR**. There are no independently launched autonomous coding agents.

**Status:** 11/70 source-scoped new acceptance items authored; 59/70 new items still open; 0/30 inherited hardware/release obligations newly accepted. **Overall 11/100 authored at source/test scope**, 89/100 not newly accepted. The R122-008 checkbox means a bounded software no-progress failure path, not a physical IPI timeout or release acceptance. Checked items are fine-grained checks for ONE integrated NIC→UDP hardening change, not independent new OS features or proven hardware functionality.

## What changed in executable kernel code

- `rust/kernel_core/src/process_retirement_txn.rs`: `ingest_nic_frame_device_fenced` now clones the bounded live socket endpoint table into a candidate, performs registered owner/frame admission on that candidate, and publishes the candidate only after exact NIC RX permit, exact BSP gate permit and exact PID ingress revision pass again.
- Three existing R122 Rust host tests exercise checked Ethernet/IPv4/UDP queue admission, checksum rejection, quiesced ring handling and stale PID ownership. Two further R122 host tests verify NIC receive admission is acquired before the actual polled virtio-net descriptor handoff, retained through callback and dropped on failure/success. One new SMP host regression checks bounded no-progress fail-closed receipt ownership. All require fresh exact-head CI.
- Production policy remains **single-BSP software staging**. In the actual QEMU UDP/DNS receiving path a single RX permit is now minted BEFORE device.poll(), owner revision is captured before poll, and the same permit is revalidated through parse and queue admission. The SMP primitive now has a bounded non-blocking-in-the-limit acknowledgement probe which never frees an unacknowledged publication. Actual DMA cessation, interrupt masking, physical IPI/TLB, IOMMU safety, physical SMP and hardware power loss are NOT proven by these source changes.

## Primary R122 70 tasks

### Workstream 1 — SMP scheduler and core correctness
- [ ] R122-001 — Implement APIC IPI transport
- [ ] R122-002 — Bind IPI receipts to exact hardware APIC IDs
- [ ] R122-003 — Acknowledge cross-core IPI completion
- [ ] R122-004 — Implement TLB shootdown acknowledgements
- [ ] R122-005 — Integrate shootdown with scheduler exit
- [ ] R122-006 — Serialize actual SMP retirement locks
- [x] R122-007 — Reject IPI receipts from wrong generation (existing generation validation + R122 negative host regression; exact-head CI pending)
- [x] R122-008 — Handle missed IPI timeout safely (bounded software no-progress result preserves outstanding receipt; hardware IPI timeout remains OPEN)
- [ ] R122-009 — Test scheduler teardown with two vCPUs
- [ ] R122-010 — Test concurrent AP startup and shutdown
- [ ] R122-011 — Audit cross-BSP kernel table reads
- [ ] R122-012 — Provide scheduler deadlock stress proof
- [ ] R122-013 — Verify physical multicore memory ordering
- [ ] R122-014 — Qualify physical SMP scheduler lifecycle

### Workstream 2 — NIC + UDP ingress and process security
- [x] R122-015 — Stage bounded UDP endpoint queue before publication (code / host regression authored; exact-head CI pending)
- [x] R122-016 — Accept authenticated Ethernet IPv4 UDP for active owner (code / host regression authored; exact-head CI pending)
- [x] R122-017 — Reject UDP checksum tampering without queue mutation (code / host regression authored; exact-head CI pending)
- [x] R122-018 — Revalidate exact NIC RX permit before queue publication (code / host regression authored; exact-head CI pending)
- [x] R122-019 — Revalidate exact BSP gate before queue publication (code / host regression authored; exact-head CI pending)
- [x] R122-020 — Revalidate PID ingress revision before queue publication (code / host regression authored; exact-head CI pending)
- [x] R122-021 — Reject quiescing NIC frames without FIFO mutation (code / host regression authored; exact-head CI pending)
- [x] R122-022 — Admit only newly issued NIC epoch after drained rebind (code / host regression authored; exact-head CI pending)
- [x] R122-023 — Preserve unrelated peer socket isolation (code / host regression authored; exact-head CI pending)
- [ ] R122-024 — Wire IRQ-time owner generation to real interrupt handler
- [ ] R122-025 — Capture NIC revision before descriptor DMA consumption
- [ ] R122-026 — Revalidate NIC revision before final IRQ UDP dispatch
- [ ] R122-027 — Mask MSI-X and stop DMA before owner retirement
- [ ] R122-028 — Qualify multi-NIC IOMMU DMA hardware isolation

### Workstream 3 — Filesystem, VFS and persistence
- [ ] R122-029 — Reject simultaneous mount replacement during path resolution
- [ ] R122-030 — Enforce selected-volume token generation
- [ ] R122-031 — Add mount stale-token mutation fuzz
- [ ] R122-032 — Verify Files queue persists after power-safe reboot
- [ ] R122-033 — Provide non-blocking retire-owner lock ordering
- [ ] R122-034 — Test filesystem and UDP teardown deadlocks
- [ ] R122-035 — Integrate monotonic storage flush clock
- [ ] R122-036 — Verify boot journal torn-write rollback
- [ ] R122-037 — Verify flush failure recovery
- [ ] R122-038 — Protect canonical rename alias during remount
- [ ] R122-039 — Check filesystem PID reuse with retained file handle
- [ ] R122-040 — Run two-cold-boot graphical Files QEMU
- [ ] R122-041 — Qualify SMP remount path lookup stress
- [ ] R122-042 — Qualify physical unsafe power loss recovery

### Workstream 4 — Boot trust, recovery and firmware
- [ ] R122-043 — Install immutable verified boot trust anchor
- [ ] R122-044 — Bind signed kernel digest to firmware measurement
- [ ] R122-045 — Reject boot epoch rollback at PID1 startup
- [ ] R122-046 — Validate bootloader provided memory maps
- [ ] R122-047 — Review IDT GDT TSS initialization boundaries
- [ ] R122-048 — Enforce kernel interrupt mask during early boot
- [ ] R122-049 — Record verified boot epoch before files mount
- [ ] R122-050 — Validate atomic journal boot epoch persistence
- [ ] R122-051 — Reject unauthorized module execution
- [ ] R122-052 — Require trusted key rotation process
- [ ] R122-053 — Verify recovery image signer
- [ ] R122-054 — Bind boot-sector restoration to trusted authority
- [ ] R122-055 — Qualify TPM secure boot measured chain
- [ ] R122-056 — Qualify actual physical boot sector restoration

### Workstream 5 — CI, QEMU and release approval
- [ ] R122-057 — Run final-SHA Rust host regression suite
- [ ] R122-058 — Run optimized no_std kernel compilation
- [ ] R122-059 — Run two-vCPU QEMU AP receipt test
- [ ] R122-060 — Run real QEMU polled virtio UDP failover
- [ ] R122-061 — Run valid device-fenced UDP end-to-end regression
- [ ] R122-062 — Run two ring3 process PID recycling QEMU
- [ ] R122-063 — Run graphical Files persistent reboot tests
- [ ] R122-064 — Run kernel ELF serial boot and ISO
- [ ] R122-065 — Run boot epoch journal failure injections
- [ ] R122-066 — Enforce non-skipped required CI checks
- [ ] R122-067 — Check exact final PR head and base identity
- [ ] R122-068 — Verify main protection checks and reviewer policy
- [ ] R122-069 — Obtain independent human kernel security review
- [ ] R122-070 — Publish post-merge main SHA with immutable evidence

## Prioritized 30 inherited R121 gates (all remain open)

- [ ] R122-I01 — R121 carry-forward: R120-013 — cross-core lock-free access protection
- [ ] R122-I02 — R121 carry-forward: R120-026 — real interrupt ingress generation ownership
- [ ] R122-I03 — R121 carry-forward: R120-028 — physical multi-NIC IRQ and IOMMU qualification
- [ ] R122-I04 — R121 carry-forward: R120-042 — cross-core IPI lifecycle ordering
- [ ] R122-I05 — R121 carry-forward: R120-070 — independent review and protected merge evidence
- [ ] R122-I06 — R121 carry-forward: R118-001 — cross-BSP mount revision synchronization
- [ ] R122-I07 — R121 carry-forward: R118-003 — concurrent mount replacement stale revision denial
- [ ] R122-I08 — R121 carry-forward: R118-009 — deterministic deadlock-free file-owner retirement
- [ ] R122-I09 — R121 carry-forward: R118-013 — selected-volume token fuzz in QEMU
- [ ] R122-I10 — R121 carry-forward: R118-014 — physical SMP remount/lookup concurrency
- [ ] R122-I11 — R121 carry-forward: R118-027 — two-BSP scheduler revocation stress
- [ ] R122-I12 — R121 carry-forward: R118-028 — actual multicore teardown and IPI safety
- [ ] R122-I13 — R121 carry-forward: R118-040 — foreign port close denial guest acceptance
- [ ] R122-I14 — R121 carry-forward: R118-041 — calibrated kernel monotonic source
- [ ] R122-I15 — R121 carry-forward: R118-042 — physical NIC ingress IRQ qualification
- [ ] R122-I16 — R121 carry-forward: R118-054 — immutable production bootloader trust anchor
- [ ] R122-I17 — R121 carry-forward: R118-055 — physical destructive power-loss recovery
- [ ] R122-I18 — R121 carry-forward: R118-056 — hardware boot-sector restoration
- [ ] R122-I19 — R121 carry-forward: R118-062 — exact-head optimized freestanding build
- [ ] R122-I20 — R121 carry-forward: R118-063 — exact-head host Rust test matrix
- [ ] R122-I21 — R121 carry-forward: R118-064 — two-boot persistent graphical Files QEMU
- [ ] R122-I22 — R121 carry-forward: R118-065 — real ring3 two-process UDP QEMU
- [ ] R122-I23 — R121 carry-forward: R118-066 — QEMU boot and network matrix
- [ ] R122-I24 — R121 carry-forward: R118-067 — independent kernel/security review
- [ ] R122-I25 — R121 carry-forward: R118-068 — exact integration SHA workflow check
- [ ] R122-I26 — R121 carry-forward: R118-070 — protected post-merge main evidence
- [ ] R122-I27 — R121 carry-forward: R119-027 — group retirement through live network table
- [ ] R122-I28 — R121 carry-forward: R119-041 — scheduler fault and normal-exit integration
- [ ] R122-I29 — R121 carry-forward: R119-056 — cross-core IRQ scheduler delivery
- [ ] R122-I30 — R121 carry-forward: R119-069 — all mandatory checks green on final source SHA

## Release checklist and unresolved blockers

1. Final-head GitHub CI (Rust host, optimized freestanding no_std, kernel ELF/ISO, QEMU network, desktop persistence, security) must pass before any merge; no prior R121 status qualifies R122.
2. The three new host tests were authored, but not compiled in this session (Rust toolchain unavailable locally). Do not mark CI green before GitHub evidence.
3. A correct BSP lifecycle permit must serialize NIC quiesce, device DMA shutdown and owner retirement. Two-phase queue staging reduces partial publication but alone cannot provide a global SMP or DMA race guarantee.
4. Physical multicore APIC/IPI/TLB, IRQ-driven NIC DMA/MSI-X/IOMMU, immutable production boot trust and unsafe hardware power loss remain open.
5. Independent human kernel/security review and strict branch protection must be verified. Automated bot approval is not independent human review.
6. The 30 inherited gates are prioritized carry-forwards, not the whole historical backlog. Continue `docs/M1_R120_100_TASK_KERNEL_RETIREMENT.md`.
