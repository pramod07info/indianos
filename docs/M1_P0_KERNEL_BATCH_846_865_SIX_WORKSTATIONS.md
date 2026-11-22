# IndianOS kernel P0 Batch35 — 20 tasks across six validation workstations

Existing draft PR #788; integration baseline `dc6cb8a41fb3bec0b2c871adb5c2de31847f92bb`. Six **GitHub Actions validation lanes**, not separate coding agents. The existing workflow suites remain enabled.

| Task | Lane | Acceptance criterion |
|---|---|---|
| P0-846 | W1 | PMM rejects aligned search on corrupt usage count |
| P0-847 | W1 | PMM refuses highest-run inference from stale bitmap |
| P0-848 | W1 | Aligned and highest run reservations remain independent |
| P0-849 | W1 | Freed highest-run becomes available after validation |
| P0-850 | W2 | APIC progress requires actual IRQ/preemption and live source |
| P0-851 | W2 | APIC status revoked after PIC handover/stop/exhaustion |
| P0-852 | W2 | Reject stale epochs and impossible preemption receipts |
| P0-853 | W3 | Allow user copies within ELF file bytes and BSS memory extent |
| P0-854 | W3 | Reject ELF page prefix and tail slack during syscall copies |
| P0-855 | W3 | Reject user copy crossing ELF allocated memory size |
| P0-856 | W3 | Retain staged zero-size segment ABI safety |
| P0-857 | W4 | Report successfully completed sectors after pinned write |
| P0-858 | W4 | Stop stale-geometry write without modifying sectors |
| P0-859 | W4 | Return partial completion after mid-write media change |
| P0-860 | W5 | Accept real ARP IPv4 Ethernet request and reply packets |
| P0-861 | W5 | Reject wrong ARP opcodes and hardware/address lengths |
| P0-862 | W5 | Reject invalid ARP hardware, protocol, and truncated frames |
| P0-863 | W6 | Validate every Limine framebuffer table slot |
| P0-864 | W6 | Reject invalid late Limine framebuffer pointer |
| P0-865 | W6 | Reject zero-count, null and oversized framebuffer tables |

## Real source changes
- PMM: `find_aligned_run` and `highest_available_run` require full bitmap accounting.
- APIC: `xapic_preemption_progress` requires active APIC clock authority instead of stale counters.
- Syscall: `user_range` limits loaded ELF memory to `p_memsz`, retaining staged ABI compatibility.
- Block I/O: caller-visible `MediaWriteFailure` exposes completed sector count on partial failure, without claiming fsync/durability; existing pinned write API delegates.
- Virtio net: RX ARP accepts only Ethernet/IPv4 request/reply fixed-size metadata; Batch34 test fixture updated to a real ARP header.
- Boot: all Limine framebuffer table entries checked before use by status, desktop and console consumers.

CI: `p0-batch35-implemented` matrix 4+3+4+3+3+3 = 20 named Rust regressions; exact head SHA + freestanding no_std target build. Prior QEMU/R028/R114/R126/M1/security checks remain enabled.

Keep draft/unmerged until exact-head CI, independent protected review and physical hardware release qualification pass. SMP/IPI/TLB shootdown, DMA/IOMMU, destructive power-loss replay, TPM and full desktop/server releases remain open.
