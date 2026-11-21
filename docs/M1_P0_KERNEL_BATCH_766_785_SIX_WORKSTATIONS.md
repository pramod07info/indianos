# Batch 31: kernel-first P0-766–785, six validation workstations

Existing PR #788, parent `41e044d147de3d9c7ecd6255b660e357a90a6733`. One consolidated commit with six **CI verification** workstations (not six separately launched coding agents).

## Previous CI regression repair
Batch30 exact head `41e044d147de3d9c7ecd6255b660e357a90a6733` showed failures in the broad M1 reliability and dedicated P0 workflows. The confirmed root in both was the inherited `kernel_p0_batch24_w1_607_corrupt_cursor_refuses_allocation` assertion: it expected malformed PMM cursor geometry to pass `verify_frame_accounting`. This batch updates the assertion to fail closed and then pass only after cursor restoration, without relaxing the production geometry check.

## 20 distinct production-linked tasks
| Workstation | Task | Implementation and test |
|---|---|---|
| W1 | P0-766 | Repair inherited corrupt-cursor PMM regression and retain fail-closed verification |
| W1 | P0-767 | Preflight exact PMM claims and span-owner lookups against full memory geometry |
| W1 | P0-768 | Reject candidate release of frames with corrupted span offsets |
| W1 | P0-769 | Refuse bitmap diagnostic snapshots with corrupt used-frame count |
| W2 | P0-770 | Tie real IRQ preemption capture to authoritative APIC timer source |
| W2 | P0-771 | Refuse PIC or missing-source APIC capture attribution |
| W2 | P0-772 | Reject stopped/saturated hardware tick and timer epoch changes |
| W3 | P0-773 | Admit original ELF entry and saved RIP only in loaded executable bytes |
| W3 | P0-774 | Reject ELF entry hidden in executable page padding |
| W3 | P0-775 | Reject ELF entry outside all executable segments |
| W3 | P0-776 | Preserve disjoint executable ELF segments while validating both entry and RIP |
| W4 | P0-777 | Return stable generation and bytes only after two verified device observations |
| W4 | P0-778 | Preserve caller buffer on late media-generation change at final publication |
| W4 | P0-779 | Deny invalid media ranges before the first read |
| W5 | P0-780 | Preserve R028 IPv4 multicast frame delivery on ingress and egress |
| W5 | P0-781 | Deny ARP multicast and preserve valid ARP Ethernet broadcast |
| W5 | P0-782 | Reject foreign unicast on RX while preserving peer-directed TX; deny unsupported multicast in both paths |
| W6 | P0-783 | Accept complete canonical Limine modules with valid borrowed file slices |
| W6 | P0-784 | Reject module spans crossing the 48-bit canonical-address hole |
| W6 | P0-785 | Reject module overflow, pointer wrapping and out-of-bounds slices |

## Acceptance
- Six exact-head GitHub Actions matrix jobs require **4+3+4+3+3+3 = 20 named Rust regression tests**, each followed by a freestanding `x86_64-unknown-none` build.
- Retain inherited 30 batch lanes and broader QEMU boot, actual R024 preemption, M1 reliable desktop/durable VFS, R028 network, PMM map, security S0 and release checks.
- **Do not claim verified or merge** until required CI passes on this exact head and independent review is complete.
- Still open: physical SMP/IPI/TLB ownership, hardware NIC IRQ/DMA/IOMMU, power-loss recovery, trusted UEFI/TPM, final page-table authorization and physical release qualification.

## Batch31 inherited regression reconciliation

A broad host run on initial commit `bf8d03c090326a5c1e027c0711dd6dea5b48fbd3` reported three inherited tests affected by stricter numeric policy: `saved_frame_uses_same_irq_policy_with_actual_executable_mapping` used a zero ELF entry despite a valid mapped executable; the test fixture now supplies the executable entry. An inherited ARP packet fixture attempted IPv4 multicast and is now identified as IPv4 multicast. The Ethernet policy also now differentiates inbound unicast ownership (foreign RX rejected) from outbound peer-directed unicast (valid TX), retaining strict multicast/ARP restrictions. No security gate or test was disabled. This amended candidate must pass full exact-head CI before review or merge.
