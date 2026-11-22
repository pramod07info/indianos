# IndianOS P0 batch25: 30 kernel tasks with six validation workstations

**Target:** open draft PR #788, branch `feat/p0-kernel-batch23-581-605-20261120`.

**Starting head:** `376bd4a62c18041aa029ec723b6108d7f978a6ab`. These changes are staged as a single follow-on commit.

Each P0 item has a distinct Rust regression case attached to an actual kernel source module. Passing the tests alone is **not** a physical hardware or release-completion claim.

| ID | Workstation | Acceptance scope | Source/test status |
|---|---|---|---|
| P0-626 | W1: PMM bitmap and C host | Healthy single-frame geometry preserves unique owner | Source staged; exact-head CI pending |
| P0-627 | W1: PMM bitmap and C host | Oversized PMM total refuses bitmap indexing | Source staged; exact-head CI pending |
| P0-628 | W1: PMM bitmap and C host | Span count above static table cannot allocate | Source staged; exact-head CI pending |
| P0-629 | W1: PMM bitmap and C host | Non-contiguous physical/bitmap offset denied without mutation | Source staged; exact-head CI pending |
| P0-630 | W1: PMM bitmap and C host | Malformed physical alignment/overflow denied; C fallback reinitialization preserves state | Source staged; exact-head CI pending |
| P0-631 | W2: Timer source and IRQ publication | Accept live PIC IRQ receipt | Source staged; exact-head CI pending |
| P0-632 | W2: Timer source and IRQ publication | Reject missing start-session IRQ receipt | Source staged; exact-head CI pending |
| P0-633 | W2: Timer source and IRQ publication | Reject revoked last-session IRQ receipt | Source staged; exact-head CI pending |
| P0-634 | W2: Timer source and IRQ publication | Reject changed epoch or source during publication | Source staged; exact-head CI pending |
| P0-635 | W2: Timer source and IRQ publication | Reject no-progress or exhausted timer session | Source staged; exact-head CI pending |
| P0-636 | W3: Ring-3 IRET and ELF pages | Allow RIP in readonly executable mapped segment | Source staged; exact-head CI pending |
| P0-637 | W3: Ring-3 IRET and ELF pages | Reject non-executable segment | Source staged; exact-head CI pending |
| P0-638 | W3: Ring-3 IRET and ELF pages | Reject writable-executable segment | Source staged; exact-head CI pending |
| P0-639 | W3: Ring-3 IRET and ELF pages | Reject code outside ELF segment and stack execution | Source staged; exact-head CI pending |
| P0-640 | W3: Ring-3 IRET and ELF pages | Reject segment count outside ABI bounds | Source staged; exact-head CI pending |
| P0-641 | W4: Removable storage media | Accept stable double-sampled capacity and generation | Source staged; exact-head CI pending |
| P0-642 | W4: Removable storage media | Reject transient sector count change | Source staged; exact-head CI pending |
| P0-643 | W4: Removable storage media | Reject transient media generation change | Source staged; exact-head CI pending |
| P0-644 | W4: Removable storage media | Deny partition mount on unstable geometry | Source staged; exact-head CI pending |
| P0-645 | W4: Removable storage media | Revoke read-only view and preserve caller output on mid-read swap | Source staged; exact-head CI pending |
| P0-646 | W5: Virtio network frame boundaries | Require exact ten-byte virtio RX offload header | Source staged; exact-head CI pending |
| P0-647 | W5: Virtio network frame boundaries | Reject oversized Ethernet-II frame before RX publication | Source staged; exact-head CI pending |
| P0-648 | W5: Virtio network frame boundaries | Retain legitimate 14-byte Ethernet-II header | Source staged; exact-head CI pending |
| P0-649 | W5: Virtio network frame boundaries | Reject multicast source MAC | Source staged; exact-head CI pending |
| P0-650 | W5: Virtio network frame boundaries | Enforce source MAC ownership on Ethernet TX | Source staged; exact-head CI pending |
| P0-651 | W6: Limine RAM and HHDM | Allow HHDM for normal low RAM | Source staged; exact-head CI pending |
| P0-652 | W6: Limine RAM and HHDM | Reject usable RAM starting at 52-bit limit | Source staged; exact-head CI pending |
| P0-653 | W6: Limine RAM and HHDM | Reject usable RAM crossing 52-bit limit | Source staged; exact-head CI pending |
| P0-654 | W6: Limine RAM and HHDM | Allow final addressable 4KiB RAM page | Source staged; exact-head CI pending |
| P0-655 | W6: Limine RAM and HHDM | Reject absent or unaligned direct-map offset | Source staged; exact-head CI pending |

## What was changed
- W1: Rust single-frame PMM validates static geometry bounds and physical span mapping before any bitmap mutation. C fallback PMM now refuses reinitialization before clearing a live allocator (the existing full-boot host test regression).
- W2: monotonic publication double-checks the session source, epoch, receipt and exhausted state after sampling.
- W3: loaded ELF plans must cover the resumed IRET RIP with an executable, non-writable page; empty-segment staged ABI tests remain allowed.
- W4: block reader/view geometry and media-generation snapshots are sampled twice to detect transient media replacement.
- W5: receive offload headers are exactly ten bytes and Ethernet ingress/destination paths enforce the physical frame bound.
- W6: the usable Limine map is rejected when any physical RAM span exceeds the maximum architectural x86-64 52-bit address bound.

## Exact-head verification requirements
- Run six `p0-batch25-implemented` Rust test lanes; each requires exactly **5** named tests, with a freestanding `x86_64-unknown-none` cargo check.
- W1 must compile/run **both** Rust-linked `make test-rust-pmm-host` and C fallback `tests/test_pmm.c` against `kernel/src/pmm.c`.
- All other repository workflows must pass against the final commit SHA, including full x86-64 kernel boot and QEMU ring-3/scheduler/fault-isolation checks.
- The previous full boot had a native C PMM assertion at `tests/test_pmm.c:75`; following QEMU gates were skipped. This code addresses the reset bug, not a verified QEMU fix.
- Keep PR #788 draft until all required checks pass and independent review/branch protection are satisfied.

## Explicitly still open
Real SMP/IPI/TLB shootdown and scheduler ownership; live installed PTE user-copy enforcement; IRQ/DMA/IOMMU qualification on physical NICs; crash-consistent journal and destructive power-loss replay; TPM verified boot; complete normal-boot desktop and protected release review.
