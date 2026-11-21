# IndianOS M1 kernel-first P0 batch32 — P0-786–805

**Integration:** existing draft PR #788, baseline `11511c379c9c59a5b8370ed8d5c8c53e1acd3afc`. **One new integration commit**, no merge before exact-head checks and protected review.

## Twenty source-backed acceptance tasks

### W1: P0-786–789 (4 new Rust regression cases)
PMM bitmap/accounting consistency before frame-set claim, release and candidate preflight. Validation: `kernel_p0_batch32_w1` with `4` cases, plus freestanding `x86_64-unknown-none` compilation.

### W2: P0-790–792 (3 new Rust regression cases)
APIC preemption readiness bound to live source and timer epoch. Validation: `kernel_p0_batch32_w2` with `3` cases, plus freestanding `x86_64-unknown-none` compilation.

### W3: P0-793–796 (4 new Rust regression cases)
strict ring-3 IRET entry/resume restricted to actual file-backed executable ELF bytes. Validation: `kernel_p0_batch32_w3` with `4` cases, plus freestanding `x86_64-unknown-none` compilation.

### W4: P0-797–799 (3 new Rust regression cases)
caller-selected media-generation atomic read with torn-transfer protection. Validation: `kernel_p0_batch32_w4` with `3` cases, plus freestanding `x86_64-unknown-none` compilation.

### W5: P0-800–802 (3 new Rust regression cases)
source-authenticated Ethernet RX destination filter and complete virtio TX receipt. Validation: `kernel_p0_batch32_w5` with `3` cases, plus freestanding `x86_64-unknown-none` compilation.

### W6: P0-803–805 (3 new Rust regression cases)
whole Limine memory-map/module/framebuffer pointer table canonical span checks. Validation: `kernel_p0_batch32_w6` with `3` cases, plus freestanding `x86_64-unknown-none` compilation.

### Per-task ledger

| ID | Workstation | Concrete implemented acceptance behavior |
|---|---|---|
| P0-786 | W1 | Exact multi-frame claim succeeds and can be atomically released |
| P0-787 | W1 | Used/bitmap mismatch rejects claim without mutating bitmap |
| P0-788 | W1 | Used/bitmap mismatch refuses release of still-owned physical frame |
| P0-789 | W1 | Claim and release candidate preflight deny corrupt accounting |
| P0-790 | W2 | Armed xAPIC with live epoch authorizes timer preemption |
| P0-791 | W2 | PIC handover or unrecognized source revokes readiness |
| P0-792 | W2 | Stopped, zero-epoch or exhausted session refuses readiness |
| P0-793 | W3 | Loaded file-backed ELF entry and IRET resume admitted |
| P0-794 | W3 | BSS/zero-fill tail cannot be resumed in strict execution |
| P0-795 | W3 | ELF entry itself in zero-fill tail cannot authorize IRET |
| P0-796 | W3 | Disjoint file-backed executable sections admitted, empty section refused |
| P0-797 | W4 | Exact expected media revision returns verified sector bytes |
| P0-798 | W4 | Stale selected generation fails without touching disk or caller bytes |
| P0-799 | W4 | Revision swap on second observation withholds caller output |
| P0-800 | W5 | RX destination filter refuses null and multicast source MAC |
| P0-801 | W5 | IPv4 multicast retained, ARP multicast and VLAN refused |
| P0-802 | W5 | Virtio TX zero/full completion accepted, partial nonzero refused |
| P0-803 | W6 | Canonical low/high Limine pointer tables admitted |
| P0-804 | W6 | Zero, excessive, null and overflowed pointer table denied |
| P0-805 | W6 | Pointer table crossing canonical gap or misaligned denied |

## Validation gates

The GitHub Actions `p0-batch32-implemented` six-workstation matrix requires **4+3+4+3+3+3 = 20 distinct Rust tests**, exact PR SHA checkout verification, and independent `no_std` freestanding compilation for each workstation. Prior P0 and inherited M1/QEMU/network/storage/security workflows stay enabled.

**Not a production release acceptance.** Real SMP/IPI/TLB hardware, physical NIC DMA/IOMMU, destructive disk power-loss recovery, TPM verified boot, full desktop qualification and independent protected code review remain open. Keep the PR draft until the *new* head's required workflows pass and review is complete.
