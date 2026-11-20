# IndianOS P0 batch 22 — twenty implemented kernel source obligations (561–580)

**Repository:** https://github.com/pramod07info/indianos

**Existing draft PR:** https://github.com/pramod07info/indianos/pull/786

**Integration policy:** One coordinated source commit. Six **GitHub Actions CI validation workstations**, not six autonomous coding agents. A source-implemented row means the indicated real Rust function was changed and regression tests were added; it does not imply QEMU or physical-hardware release completion. Mark host acceptance only after exact-head CI passes.

## Twenty source obligations and acceptance evidence

### W1: Physical memory allocator self-test recovery (P0-561–563)

Source: `rust/kernel_core/src/pmm.rs` — dedicated test filter: `kernel_p0_batch22_w1`.

PMM is BSP-only. This hardens the C-visible self-test contract, not real AP allocation synchronization.

| Task | Live-source change | Exact acceptance |
|---|---|---|
| P0-561 | Reclaim first allocated frame when the second PMM self-test allocation fails | Injected allocation failure leaves all test-owned frames free. Rust test name `kernel_p0_batch22_w1_561_...` |
| P0-562 | Reject a duplicated physical frame returned during self-test without double-release | Aliased allocation is rejected and no frame is freed twice. Rust test name `kernel_p0_batch22_w1_562_...` |
| P0-563 | Require full two-frame allocate/recycle/reclaim roundtrip before claiming PMM self-test success | Repeated healthy roundtrips restore all owner counters. Rust test name `kernel_p0_batch22_w1_563_...` |

### W2: Live APIC/PIC IRQ receipt authentication (P0-564–566)

Source: `rust/kernel_core/src/timer.rs` — dedicated test filter: `kernel_p0_batch22_w2`.

Reuses existing timer-source epochs from P0-488/528. Actual cross-vCPU monotonic clock calibration remains open.

| Task | Live-source change | Exact acceptance |
|---|---|---|
| P0-564 | Stamp successfully delivered real IRQ progress with its timer session epoch | Only matching epoch receipt authorizes cumulative clock reads. Rust test name `kernel_p0_batch22_w2_564_...` |
| P0-565 | Reject stale same-source rearm receipts and zero-epoch receipt claims | Changed epoch and no session progress produce no time authority. Rust test name `kernel_p0_batch22_w2_565_...` |
| P0-566 | Deny source conflict and exhausted counters despite valid epoch receipt | PIC/APIC conflict and permanently exhausted ticks fail closed. Rust test name `kernel_p0_batch22_w2_566_...` |

### W3: Ring-3 trap-frame unsupported flag policy (P0-567–570)

Source: `rust/kernel_core/src/syscall.rs` — dedicated test filter: `kernel_p0_batch22_w3`.

Actual mapped-user-PTE admission and syscall copyin/copyout remain separate larger tasks.

| Task | Live-source change | Exact acceptance |
|---|---|---|
| P0-567 | Reject direction flag DF in user IRET context until safe trap semantics exist | Actual ring3_frame_valid refuses DF without breaking clean RFLAGS. Rust test name `kernel_p0_batch22_w3_567_...` |
| P0-568 | Reject trap flag TF in user IRET context without dedicated debug trap machinery | Actual ring3_frame_valid rejects trap-flag reentry. Rust test name `kernel_p0_batch22_w3_568_...` |
| P0-569 | Reject alignment-check flag AC from user return snapshots | Bad AC frame is not resumed. Rust test name `kernel_p0_batch22_w3_569_...` |
| P0-570 | Reject resume flag RF while preserving normally enabled user interrupts | RF denied and RFLAGS 0x202 accepted only for enabled-IRQ profile. Rust test name `kernel_p0_batch22_w3_570_...` |

### W4: Generation-bound disk I/O across four real paths (P0-571–574)

Source: `rust/kernel_core/src/block.rs` — dedicated test filter: `kernel_p0_batch22_w4`.

Uses opt-in BlockDevice::media_generation() added in the previous batch; physical driver tokens remain unqualified until drivers supply real epochs.

| Task | Live-source change | Exact acceptance |
|---|---|---|
| P0-571 | Bind bulk sector reads to one media generation | Mid-read replacement produces Io and zeroes caller buffer. Rust test name `kernel_p0_batch22_w4_571_...` |
| P0-572 | Bind multi-sector writes to one media generation | After write-time swap, stop further sectors; return uncertain Io. Rust test name `kernel_p0_batch22_w4_572_...` |
| P0-573 | Bind atomic metadata-read publication to one media generation | Swap leaves all caller bytes unchanged. Rust test name `kernel_p0_batch22_w4_573_...` |
| P0-574 | Bind dual-observation stable-metadata reads to one media generation | Swap between reads denied; stable device still succeeds. Rust test name `kernel_p0_batch22_w4_574_...` |

### W5: Live virtio-net Ethernet ingress filtering (P0-575–577)

Source: `rust/kernel_core/src/virtio_net.rs` — dedicated test filter: `kernel_p0_batch22_w5`.

R028 QEMU-only polled legacy virtio-net, checksum offload disabled. Hardware DMA/IOMMU and live IRQ fencing remain unqualified.

| Task | Live-source change | Exact acceptance |
|---|---|---|
| P0-575 | Reject Ethernet frames whose source address is multicast or broadcast | Device poll denies frame before caller-copy; positive unicast test. Rust test name `kernel_p0_batch22_w5_575_...` |
| P0-576 | Reject an all-zero source MAC and frames lacking a full Ethernet header | Invalid DMA payload cannot become an upper-layer packet. Rust test name `kernel_p0_batch22_w5_576_...` |
| P0-577 | Reject unsupported IEEE 802.3 length-coded frames on Ethernet-II-only transport | IPv4/ARP EtherTypes accepted; length-coded input denied. Rust test name `kernel_p0_batch22_w5_577_...` |

### W6: Limine boot pointer and HHDM-range preflight (P0-578–580)

Source: `rust/kernel_core/src/boot.rs` — dedicated test filter: `kernel_p0_batch22_w6`.

Bootloader-owned memory validity and TPM verified boot cannot be established from arithmetic unit tests alone.

| Task | Live-source change | Exact acceptance |
|---|---|---|
| P0-578 | Reject misaligned Limine entry-pointer arrays before volatile reads | Adversarial misalignment fails without dereferencing pointer. Rust test name `kernel_p0_batch22_w6_578_...` |
| P0-579 | Reject misaligned Limine region pointers before as_ref | Corrupt pointer cannot produce invalid Rust reference during preflight. Rust test name `kernel_p0_batch22_w6_579_...` |
| P0-580 | Reject HHDM offsets whose mapped high-half cannot hold all usable physical spans | Overflowing physical region denied before normal boot initialization. Rust test name `kernel_p0_batch22_w6_580_...` |

## Six independent source validation lanes

| Workstation | Expected new tests | Production file | Feature |
|---|---:|---|---|
| W1 | 3 | `pmm.rs` | `normal-desktop` |
| W2 | 3 | `timer.rs` | `normal-desktop` |
| W3 | 4 | `syscall.rs` | `normal-desktop` |
| W4 | 4 | `block.rs` | `test-r031-vfs` |
| W5 | 3 | `virtio_net.rs` | `test-r028-network` |
| W6 | 3 | `boot.rs` | `normal-desktop` |
| **Total** | **20** | Six source modules | |

The new workflow requires **exactly the expected number of regression tests** in each lane, runs each Rust filter, and compiles the no_std freestanding x86-64 kernel for the relevant feature. It also pins checked-out code to the exact PR head.

### Source completion is not hardware completion

These 20 items are narrowly scoped, concrete source-level hardening tasks. The next actual release-blocking kernel gates are still open: SMP allocator lock ownership; APIC/TLB IPI acknowledgments; real ring3 PTE and copyin/copyout; filesystem journaling and destructive power-loss; physical driver media-epoch reporting; NIC IRQ/DMA/IOMMU lifetime; TPM/UEFI signed boot; normal desktop usability; independent protected-branch review. Never mark those inherited kernel release tasks complete merely because this batch passes. Keep draft PR unmerged until release gates are independently accepted.
