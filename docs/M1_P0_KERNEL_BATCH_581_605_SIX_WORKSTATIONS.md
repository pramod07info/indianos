# IndianOS P0 batch 23 — 25 prioritized kernel-source tasks (P0-581–605)

**Baseline**: merged PR #786, main commit `ffa99c46c8d3d3164b1edf936cbe55226f250e7f` (verified merged)
  
**Integration policy**: new branch `feat/p0-kernel-batch23-581-605-20261120`; one production integration commit, one new draft PR targeting `main`. Six **GitHub Actions CI test workstations**, not independently running coding agents.

The 25 tasks below are **narrow kernel-source obligations** supporting six P0 safety workstreams. They are implemented in existing production Rust functions and each has a test. **Mark their source-level acceptance only after exact-head CI succeeds.** They do not close the larger physical-device, SMP, reliable storage, protected merge or shipping gates.

## Prioritized implementation and acceptance ledger

### W1: PMM frame-set accounting (BSP-only) (P0-581–584)

Production path: `rust/kernel_core/src/pmm.rs`; exact Rust test filter `kernel_p0_batch23_w1`. Live discontiguous exact frame ownership claims/releases. This is not concurrent SMP allocator safety.

| ID | Priority implementation | Source-level acceptance |
|---|---|---|
| P0-581 | Reject corrupt used>total during multi-frame claim | No bitmap state or cursor changes. Dedicated test `kernel_p0_batch23_w1_581_...` |
| P0-582 | Reject multi-frame claims exceeding remaining accounting budget | No partial bitmap claim across holes or insufficient capacity. Dedicated test `kernel_p0_batch23_w1_582_...` |
| P0-583 | Reject discontiguous release when used is less than release count | Busy bitmap stays intact; no unsigned underflow. Dedicated test `kernel_p0_batch23_w1_583_...` |
| P0-584 | Allow healthy discontiguous claim/release with double-free rejection | Frame accounting returns to zero and bitmap audit passes. Dedicated test `kernel_p0_batch23_w1_584_...` |

### W2: Timer IRQ authority and source switchover (P0-585–588)

Production path: `rust/kernel_core/src/timer.rs`; exact Rust test filter `kernel_p0_batch23_w2`. The real monotonic tick publication requires its hardware controller to be configured; cross-CPU timer calibration is still open.

| ID | Priority implementation | Source-level acceptance |
|---|---|---|
| P0-585 | Reject PIC tick receipt while IRQ0 is not armed | No monotonic progress from stopped PIC. Dedicated test `kernel_p0_batch23_w2_585_...` |
| P0-586 | Prevent stale PIC receipt overriding active xAPIC | Running APIC takes authority over legacy PIC. Dedicated test `kernel_p0_batch23_w2_586_...` |
| P0-587 | Reject xAPIC tick receipt unless APIC timer is running | Configured PIC alone never proves xAPIC IRQ. Dedicated test `kernel_p0_batch23_w2_587_...` |
| P0-588 | Reject unknown timer source numbers | Only currently implemented PIC and xAPIC sources may publish ticks. Dedicated test `kernel_p0_batch23_w2_588_...` |

### W3: Ring3 stack/guard and syscall buffer layout (P0-589–592)

Production path: `rust/kernel_core/src/syscall.rs`; exact Rust test filter `kernel_p0_batch23_w3`. Numeric syscall/IRET admission rejects malformed stack/guard and segment layout. Actual PTE/CPU isolation remains unqualified.

| ID | Priority implementation | Source-level acceptance |
|---|---|---|
| P0-589 | Reject malformed nonzero guard metadata while preserving staged zero-guard plans | Unaligned nonzero guard fails; zero-guard staging remains valid only subject to actual page-table checks. Dedicated test `kernel_p0_batch23_w3_589_...` |
| P0-590 | Reject guard overlap or misalignment with live user stack | No syscall copy into a malformed stack layout. Dedicated test `kernel_p0_batch23_w3_590_...` |
| P0-591 | Reject unaligned and truncated ELF segment page intervals | Malformed segment cannot authorize a copy buffer. Dedicated test `kernel_p0_batch23_w3_591_...` |
| P0-592 | Preserve legitimate stack and data-segment access | Usable aligned guard and segment continue to work. Dedicated test `kernel_p0_batch23_w3_592_...` |

### W4: Mounted extent identity and one-way revocation (P0-593–597)

Production path: `rust/kernel_core/src/block.rs`; exact Rust test filter `kernel_p0_batch23_w4`. A real mounted ExtentReader now pins opt-in BlockDevice media generation and retires the mount after driver failure. Physical drivers must supply true epochs.

| ID | Priority implementation | Source-level acceptance |
|---|---|---|
| P0-593 | Capture device media generation when mounting extents | Read-only extent reports its pinned identity token. Dedicated test `kernel_p0_batch23_w4_593_...` |
| P0-594 | Deny a same-capacity media replacement before reading any caller bytes | No buffer publication after identity mismatch. Dedicated test `kernel_p0_batch23_w4_594_...` |
| P0-595 | Permanently revoke the extent reader after any media replacement | Restoring the old token cannot revive a stale mount. Dedicated test `kernel_p0_batch23_w4_595_...` |
| P0-596 | Retire an extent reader after driver I/O failure | A dirty failed bounce buffer never escapes; no retry into uncertain media. Dedicated test `kernel_p0_batch23_w4_596_...` |
| P0-597 | Prohibit write and sync through read-only extent adapter | No mutation or durability acknowledgment from read-only extent. Dedicated test `kernel_p0_batch23_w4_597_...` |

### W5: Virtio TX DMA provenance (P0-598–601)

Production path: `rust/kernel_core/src/virtio_net.rs`; exact Rust test filter `kernel_p0_batch23_w5`. The live legacy virtio Ethernet send path rejects source spoofing and unsupported frames before making DMA descriptors available.

| ID | Priority implementation | Source-level acceptance |
|---|---|---|
| P0-598 | Accept properly attributed Ethernet-II ARP frames from local NIC MAC | Healthy QEMU ARP path retains outbound access. Dedicated test `kernel_p0_batch23_w5_598_...` |
| P0-599 | Reject source-MAC spoofing before transmit descriptor submission | Cannot send forged source identity. Dedicated test `kernel_p0_batch23_w5_599_...` |
| P0-600 | Reject zero destination MAC while preserving valid unicast routing | No unspecified-address egress. Dedicated test `kernel_p0_batch23_w5_600_...` |
| P0-601 | Reject length-coded and truncated frames on Ethernet-II-only transport | No unsupported Layer-2 frames submitted for DMA. Dedicated test `kernel_p0_batch23_w5_601_...` |

### W6: Limine HHDM and boot metadata safeguards (P0-602–605)

Production path: `rust/kernel_core/src/boot.rs`; exact Rust test filter `kernel_p0_batch23_w6`. The boot_hhdm_offset exported API now validates the complete memory-map mapping and aligned Limine module/framebuffer pointers.

| ID | Priority implementation | Source-level acceptance |
|---|---|---|
| P0-602 | Validate the complete usable memory map before exporting HHDM offset | Reject direct-map overflow in the existing API. Dedicated test `kernel_p0_batch23_w6_602_...` |
| P0-603 | Validate Limine module pointer-table alignment before volatile loads | Misaligned module table cannot be dereferenced. Dedicated test `kernel_p0_batch23_w6_603_...` |
| P0-604 | Validate Limine framebuffer pointer-table alignment before volatile loads | Unsafe framebuffer table never enters drawing. Dedicated test `kernel_p0_batch23_w6_604_...` |
| P0-605 | Reject misaligned module file pointer before dereferencing archive metadata | Invalid file pointer is not admitted. Dedicated test `kernel_p0_batch23_w6_605_...` |

**Kernel staging compatibility:** A zero guard address is permitted in synthetic/staged kernel plans; it is not evidence that user memory is mapped or authorized. Installed user PTEs must still be validated. Nonzero guards are checked for alignment and non-overlap.

## Workstation and compile evidence

| Workstation | Exact test count | Rust target |
|---|---:|---|
| W1 — PMM | 4 | normal-desktop |
| W2 — timer | 4 | normal-desktop |
| W3 — user isolation | 4 | normal-desktop |
| W4 — extent storage | 5 | test-r031-vfs |
| W5 — legacy virtio-net | 4 | test-r028-network |
| W6 — Limine boot | 4 | normal-desktop |
| **Total** | **25** | x86_64-unknown-none freestanding no_std compilation |

Each CI lane must run nonempty Rust tests, assert the exact count for its workstream, and compile the actual `no_std` freestanding kernel target at the PR head. Other legacy and QEMU workflows remain mandatory.

## Larger kernel-first P0 priorities remain open

1. True SMP/AP-core allocator exclusivity, IPI-backed TLB shootdowns and frame-reclaim generation ownership.
2. Live ring3 installed PTE checks, reliable fault containment and verified all-or-none syscall copy.
3. Filesystem journal ordering with replay and destructive power-loss qualification on a QEMU disk.
4. Physical NIC IRQ/MSI-X draining, DMA/IOMMU fencing and hardware device media-generation reporting.
5. TPM/UEFI verified boot and rollback prevention, final QEMU desktop usability and independent protected-branch review.

The current tasks improve selected production safety boundaries but do **not** prove any of these five major requirements. Keep the new PR draft and unmerged until full CI and appropriate release acceptance.
