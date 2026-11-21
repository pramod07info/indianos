# IndianOS P0 Batch33 — Tasks P0-806–825 / six validation workstations

**Integration baseline**: `eef9ca027107c1da3ff183be9449d0afc0e84af2` in existing draft PR #788; one integration commit. The six GitHub Actions matrix lanes are validation workstations, **not six independently spawned coding agents**.

## Implemented acceptance ledger

| Task | Workstation | Rust-source acceptance criterion |
|---|---| 
| P0-806 | W1 | Refuse exact run claim when bitmap accounting is inconsistent |
| P0-807 | W1 | Preserve owned physical frames when release accounting is inconsistent |
| P0-808 | W1 | Deny contiguous multi-frame DMA allocation on corrupt bitmap usage |
| P0-809 | W1 | Verify aligned, highest and contiguous run claim and release roundtrip |
| P0-810 | W2 | Accept an active xAPIC session with a valid IRQ capture receipt |
| P0-811 | W2 | Revoke APIC preemption when monotonic clock is exhausted |
| P0-812 | W2 | Reject PIC handover, stale epoch, and exhausted IRQ delivery budget |
| P0-813 | W3 | Accept strict ring-3 IRET with configured stack and valid ELF code |
| P0-814 | W3 | Reject widened or shrunken strict execution stack |
| P0-815 | W3 | Reject misaligned saved RSP in strict ring-3 execution |
| P0-816 | W3 | Accept stack lower bound while rejecting guard and missing guard |
| P0-817 | W4 | Publish two-pass disk read only with expected capacity and revision |
| P0-818 | W4 | Reject changed capacity in read preflight without I/O or byte leakage |
| P0-819 | W4 | Withhold sector bytes after capacity swap during double read |
| P0-820 | W5 | Validate complete no-offload virtio frame before RX publication |
| P0-821 | W5 | Reject inconsistent used-ring length and unsupported offload metadata |
| P0-822 | W5 | Deny foreign-destination and invalid-source Ethernet ingress |
| P0-823 | W6 | Admit aligned RAM in the 52-bit physical HHDM range |
| P0-824 | W6 | Refuse usable frame zero, reserved RAM, and beyond-MAXPHYADDR spans |
| P0-825 | W6 | Deny unaligned, wrapping and noncanonical HHDM spans |

### Workstation ownership and tests

- **W1 / PMM P0-806–809**: `pmm.rs`, four `kernel_p0_batch33_w1` cases. Frame-run claim and release are bitmap-accounting fenced.
- **W2 / APIC P0-810–812**: `timer.rs`, three `kernel_p0_batch33_w2` cases. Preemption readiness and receipt recording honor sticky exhaustion.
- **W3 / ring-3 P0-813–816**: `syscall.rs`, four `kernel_p0_batch33_w3` cases. Strict-only IRET requires configured fixed-size stack and 8-byte saved RSP alignment; compatibility staging ABI stays separate.
- **W4 / storage P0-817–819**: `block.rs`, three `kernel_p0_batch33_w4` cases. Caller pins both device capacity and revision and gets no bytes before atomic double-read acceptance.
- **W5 / network P0-820–822**: `virtio_net.rs`, three `kernel_p0_batch33_w5` cases. Shared RX frame validation handles used length, offload header, protocol, source and MAC address before copying to caller.
- **W6 / Limine boot P0-823–825**: `boot.rs`, three `kernel_p0_batch33_w6` cases. HHDM usable region preflight rejects invalid physical and virtual address spans before PMM handoff.

CI job `p0-batch33-implemented` must discover **4+3+4+3+3+3=20 named test cases**, run each matched Rust test set against exact PR SHA, and compile the freestanding `x86_64-unknown-none` target. All prior P0 and inherited QEMU/boot/security/M1 workflows remain enabled. CI success must be observed on the current commit, not inherited from earlier commits.

## Release safety

Still **unqualified**: physical SMP/IPI/TLB shootdowns, physical NIC DMA/IOMMU, destructive power-loss recovery, TPM verified boot, complete usable desktop, independent protected review and hardware release. Preserve PR draft status; no merge before all exact-head mandatory checks and independent review pass.
