# IndianOS P0 batch26 — 20 kernel-first tasks, six test workstations

**Repository:** pramod07info/indianos
**Target draft PR:** #788
**Base integration head:** `7499f4e00b8bae1df82621238844246ce01ec5dd`
**Implementation scope:** actual kernel source functions, per-task Rust regressions, six CI validation workstations (not claims of six independently running coding agents).

| Task | Lane | Component | Acceptance obligation | Status |
|---|---|---|---|---|
| P0-656 | W1 | Contiguous PMM | Allow exact two-frame contiguous allocation | Implemented in source; exact-head CI pending |
| P0-657 | W1 | Contiguous PMM | Reject invalid span offsets before DMA-run allocation | Implemented in source; exact-head CI pending |
| P0-658 | W1 | Contiguous PMM | Reject corrupt used-frame accounting without mutation | Implemented in source; exact-head CI pending |
| P0-659 | W1 | Contiguous PMM | Find a free run after a legitimately allocated page | Implemented in source; exact-head CI pending |
| P0-660 | W2 | Timer publication | Publish a live PIC session with stable final receipt | Implemented in source; exact-head CI pending |
| P0-661 | W2 | Timer publication | Reject published ticks after PIC shutdown | Implemented in source; exact-head CI pending |
| P0-662 | W2 | Timer publication | Revoke stale PIC time during APIC switchover | Implemented in source; exact-head CI pending |
| P0-663 | W3 | User-memory isolation | Accept independent executable segment and writable user stack | Implemented in source; exact-head CI pending |
| P0-664 | W3 | User-memory isolation | Reject overlapping ELF segment ownership during user-copy/IRET | Implemented in source; exact-head CI pending |
| P0-665 | W3 | User-memory isolation | Reject executable/stack admission via guard-page alias | Implemented in source; exact-head CI pending |
| P0-666 | W4 | Read-only media probe | Allow stable read-only discovery read | Implemented in source; exact-head CI pending |
| P0-667 | W4 | Read-only media probe | Reject sector-count drift on first probe before I/O | Implemented in source; exact-head CI pending |
| P0-668 | W4 | Read-only media probe | Reject generation drift on first probe | Implemented in source; exact-head CI pending |
| P0-669 | W4 | Read-only media probe | Keep revoked probe permanently denied and caller buffer intact | Implemented in source; exact-head CI pending |
| P0-670 | W5 | Ethernet framing | Retain untagged IPv4 and ARP | Implemented in source; exact-head CI pending |
| P0-671 | W5 | Ethernet framing | Reject VLAN-tagged RX on untagged-only ABI | Implemented in source; exact-head CI pending |
| P0-672 | W5 | Ethernet framing | Reject VLAN-tagged TX on untagged-only ABI | Implemented in source; exact-head CI pending |
| P0-673 | W6 | Limine RAM preflight | Allow page-aligned usable physical RAM | Implemented in source; exact-head CI pending |
| P0-674 | W6 | Limine RAM preflight | Reject usable RAM beginning off a 4KiB boundary | Implemented in source; exact-head CI pending |
| P0-675 | W6 | Limine RAM preflight | Reject usable RAM ending mid-page | Implemented in source; exact-head CI pending |

## Integration and acceptance
- Six `p0-batch26-implemented` CI matrix entries, test counts **4+3+3+4+3+3 = 20**, each also compiling the freestanding `x86_64-unknown-none` kernel.
- Source paths: `pmm.rs`, `timer.rs`, `syscall.rs`, `block.rs`, `virtio_net.rs`, `boot.rs`; C PMM host regression remains enabled in CI.
- Do not mark as CI-passed before exact final commit SHA tests finish. Full kernel QEMU boot, multi-CPU/IRQ/DMA/IOMMU, live PTE permission checks, destructive journal power-loss, TPM and independent protected-branch review remain **OPEN**.
- Keep #788 draft, use one integration commit for this 20-task batch. No merge until independent review and full exact-head CI.
