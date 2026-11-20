# IndianOS P0 batch 24 — 20 kernel-first source tasks (P0-606–625)

**Existing draft PR:** https://github.com/pramod07info/indianos/pull/788

**Baseline for the amended integration commit:** merged main `ffa99c46c8d3d3164b1edf936cbe55226f250e7f`. Retain one consolidated source integration commit on the existing feature branch and keep PR #788 draft.

**Prior exact-head status:** 12/13 repository workflows passed at earlier SHA `7428c9a`; the full kernel boot workflow failed. Logs identified an outdated Rust PMM C host test expecting reset-on-reinitialize and an additional QEMU preemption test failure. This batch updates the PMM ABI test without weakening the one-shot allocator. The QEMU preemption issue remains OPEN unless the new exact-head workflow independently proves otherwise.

**Six workstations = six independent CI validation lanes, NOT six parallel autonomous coding agents.** Exactly 20 new Rust regression cases are required, plus the real C ABI host binary in W1.

## Implementation tasks and acceptance criteria

### W1: One-shot physical memory ownership and C ABI recovery (P0-606–609)

Production Rust path: `rust/kernel_core/src/pmm.rs`; supporting C host test: `tests/test_pmm.c`. Existing pmm_initialize intentionally refuses replacing an initialized allocator. The old C host test expected its reset and previously failed the full kernel-boot workflow.

| ID | Source work | Source-level acceptance |
|---|---|---|
| P0-606 | Keep live PMM state after rejected reinitialize | Update real C ABI host test to assert existing 8-frame allocator survives invalid/valid reinitialization attempts; Rust one-shot test confirms frame ownership. Dedicated Rust test name starts `kernel_p0_batch24_w1_606_` |
| P0-607 | Reject corrupt allocator cursor before changing bitmap | State::claim_single_frame refuses cursor==total without frame allocation. Dedicated Rust test name starts `kernel_p0_batch24_w1_607_` |
| P0-608 | Reject impossible used>total frame accounting | Impossible counter state returns None, no partial mutation. Dedicated Rust test name starts `kernel_p0_batch24_w1_608_` |
| P0-609 | Verify one-shot exhaustion, recycle and bitmap accounting | After exact exhaustion, freed frame is reused without alias; ownership audit passes. Dedicated Rust test name starts `kernel_p0_batch24_w1_609_` |

### W2: IRQ monotonic receipt safety across source switches (P0-610–612)

Production Rust path: `rust/kernel_core/src/timer.rs`. Monotonic clock remains BSP/source-based; cross-CPU calibration and real IPI timer ownership remain unqualified.

| ID | Source work | Source-level acceptance |
|---|---|---|
| P0-610 | Require current PIC/APIC session before incrementing monotonic tick | Changed session epoch and simultaneous PIC/APIC authority refuse receipt. Dedicated Rust test name starts `kernel_p0_batch24_w2_610_` |
| P0-611 | Reject stopped or unrecognized IRQ source on both sides of increment | Stopped PIC/APIC and mismatched source refuse progress. Dedicated Rust test name starts `kernel_p0_batch24_w2_611_` |
| P0-612 | Never reauthorize exhausted or wrapped monotonic clock | Exhausted tick counter and wrapped epoch cannot stamp new time. Dedicated Rust test name starts `kernel_p0_batch24_w2_612_` |

### W3: Ring-3 control-flow numeric admission (P0-613–615)

Production Rust path: `rust/kernel_core/src/syscall.rs`. Live PTE checks and actual ring3 CPU NX isolation remain separate acceptance gates.

| ID | Source work | Source-level acceptance |
|---|---|---|
| P0-613 | Reject unaligned user stack top in trap-frame admission | IRETQ candidate with unaligned stack_end fails; aligned control passes. Dedicated Rust test name starts `kernel_p0_batch24_w3_613_` |
| P0-614 | Deny saved ring3 RIP inside the user stack | No numeric return directly to stack page; ordinary executable RIP remains allowed. Dedicated Rust test name starts `kernel_p0_batch24_w3_614_` |
| P0-615 | Deny saved ring3 RIP in a configured guard page | Guard RIP is rejected while zero-guard staged image keeps its previous numeric contract. Dedicated Rust test name starts `kernel_p0_batch24_w3_615_` |

### W4: Revoked storage view must not advertise usable media token (P0-616–619)

Production Rust path: `rust/kernel_core/src/block.rs`. Driver media_generation() remains opt-in; physical ATA/virtio drivers still need real reset/hotplug identity qualification.

| ID | Source work | Source-level acceptance |
|---|---|---|
| P0-616 | Clear reported generation after read-only device revocation | After identity swap and failed read, stale generation is not offered. Dedicated Rust test name starts `kernel_p0_batch24_w4_616_` |
| P0-617 | Clear reported generation after bounded-window write failure | Torn/failed write permanently invalidates mounted window and generation. Dedicated Rust test name starts `kernel_p0_batch24_w4_617_` |
| P0-618 | Clear reported generation after extent-mount media replacement | Restoring old driver token cannot revive stale extent. Dedicated Rust test name starts `kernel_p0_batch24_w4_618_` |
| P0-619 | Retain pinned generation only while view remains healthy | Successful selected device read maintains original epoch and bytes. Dedicated Rust test name starts `kernel_p0_batch24_w4_619_` |

### W5: Virtio RX destination ownership (P0-620–622)

Production Rust path: `rust/kernel_core/src/virtio_net.rs`. R028 legacy polled NIC only. Physical DMA/IOMMU, MSI-X and RX reset-generation ownership remain open.

| ID | Source work | Source-level acceptance |
|---|---|---|
| P0-620 | Accept local unicast Ethernet frames in actual NIC poll | Destination equal to live NIC MAC and valid EtherType passes. Dedicated Rust test name starts `kernel_p0_batch24_w5_620_` |
| P0-621 | Reject foreign unicast frame before caller buffer publication | Wrong unicast destination yields invalid RX without copying bytes. Dedicated Rust test name starts `kernel_p0_batch24_w5_621_` |
| P0-622 | Preserve discovery broadcast and multicast traffic | ARP broadcast and multicast remain eligible, while short frames are denied. Dedicated Rust test name starts `kernel_p0_batch24_w5_622_` |

### W6: Safe Limine response pointer admission before reference construction (P0-623–625)

Production Rust path: `rust/kernel_core/src/boot.rs`. Bootloader memory lifetime/provenance and TPM/UEFI signed boot remain unqualified.

| ID | Source work | Source-level acceptance |
|---|---|---|
| P0-623 | Refuse missing memory-map/HHDM response pointer | Null Limine response is rejected without dereference. Dedicated Rust test name starts `kernel_p0_batch24_w6_623_` |
| P0-624 | Reject misaligned Limine response pointers | Misaligned metadata fails before unsafe as_ref. Dedicated Rust test name starts `kernel_p0_batch24_w6_624_` |
| P0-625 | Permit properly aligned response references for valid boot handoff | Valid locally owned aligned HHDM/MemoryMap entries remain readable. Dedicated Rust test name starts `kernel_p0_batch24_w6_625_` |

## Exact-head source CI

| Workstation | New Rust tests | Feature |
|---|---:|---|
| W1 PMM + C ABI host | 4 + `make test-rust-pmm-host` | `normal-desktop` |
| W2 timer | 3 | `normal-desktop` |
| W3 ring3 | 3 | `normal-desktop` |
| W4 storage | 4 | `test-r031-vfs` |
| W5 virtio RX | 3 | `test-r028-network` |
| W6 boot | 3 | `normal-desktop` |
| **Total** | **20 Rust cases plus real PMM C host binary** | `x86_64-unknown-none` freestanding compilation |

All six CI jobs checkout the exact PR head SHA, enforce nonempty test counts, run actual Rust tests and compile the no_std kernel. **Do not mark the batch verified until exact-head CI completes**, and do not attribute a source-level pass to physical hardware.

## Still open for kernel/release qualification

- Actual cross-vCPU PMM allocator locking, IPI/TLB shootdown, retirement generations.
- Real installed PTE and runtime user-copy safety, supervisor/user fault isolation.
- Journal power-loss recovery, verified persistent desktop and Files survival.
- Physical NIC DMA/IOMMU and driver media-generation reporting.
- Signed/TPM boot, independent protected-branch review, all required final-head CI including QEMU preemption.
