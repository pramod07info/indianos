# IndianOS P0 Batch34 — Tasks P0-826..845 / six validation workstations

Base of work: `d7b8af1d24e43bb0642d279aa4c68bb5f6bafa77` (Batch33 repair). One new integration commit in existing draft PR #788; six GitHub Actions validation lanes, not independently spawned development agents.

## 20 distinct kernel-first tasks
| ID | Lane | Acceptance criterion |
|---|---|---|
| P0-826 | W1 | Build atomic two-span PMM occupancy snapshot |
| P0-827 | W1 | Reject undersized snapshot destination without publication |
| P0-828 | W1 | Refuse bitmap-accounting corruption without disclosing partial span data |
| P0-829 | W1 | Keep span fragmentation metrics coherent after release |
| P0-830 | W2 | Admit live APIC preemption receipt with no exhaustion |
| P0-831 | W2 | Refuse exhaustion on either side of preemption capture |
| P0-832 | W2 | Reject epoch switch, source takeover and timer disarm |
| P0-833 | W3 | Treat first and last file-backed ELF code bytes as executable |
| P0-834 | W3 | Reject code access into zero-filled BSS and before segment start |
| P0-835 | W3 | Refuse writable-executable and non-executable segment admission |
| P0-836 | W3 | Reject overflow, zero file bytes, or file-size beyond memory |
| P0-837 | W4 | Write all sectors on pinned device capacity and generation |
| P0-838 | W4 | Refuse stale geometry or revision before any media write |
| P0-839 | W4 | Abort multi-sector write on detected media revision swap |
| P0-840 | W5 | Require minimum IPv4 header before accepting virtio RX frame |
| P0-841 | W5 | Require complete ARP header before accepting virtio RX frame |
| P0-842 | W5 | Deny unsupported EtherType and truncated Ethernet-II frame |
| P0-843 | W6 | Prove centered or full framebuffer rectangle fits physical pitch |
| P0-844 | W6 | Deny out-of-bounds and overflowing boot framebuffer rectangles |
| P0-845 | W6 | Deny framebuffer rectangles on malformed pitch or color masks |

## Production source changes
- **W1** `pmm.rs`: Stage all span usage structures on the stack and publish to caller only on complete successful bitmap-accounting validation.
- **W2** `timer.rs`: Carry monotonic clock exhaustion observations from both sides of IRQ preemption capture, in addition to source and epoch authority.
- **W3** `syscall.rs`: Central checked ELF instruction ownership helper for strict ring-3 IRET admission. Only file-backed, non-writable executable bytes qualify, not BSS or overflow.
- **W4** `block.rs`: New `write_expected_media_snapshot_sectors` for writes tied to the previously selected device capacity and generation, with pre- and post-sector checks; partial writes cannot be rolled back after media swap.
- **W5** `virtio_net.rs`: RX validates minimum IPv4 or ARP protocol header length before publishing packets; TX policy unchanged.
- **W6** `boot.rs`: Validate framebuffer status rectangle, pitch, bounds, channel masks and overflow before volatile draw.

Six new exact-PR-head CI jobs run **4+3+4+3+3+3=20** named Rust regression tests on real module code, plus freestanding `x86_64-unknown-none` build. All inherited P0 batches and R028/R114/R126/M1/QEMU/security workflows remain enabled.

## Evidence and release policy
Code-push alone does not mean tests have passed. Preserve PR #788 **draft** until all mandatory exact-head checks pass and independent protected review and physical release qualification are complete. Physical SMP/IPI/TLB ownership, DMA/IOMMU, destructive power-loss recovery, TPM, full desktop release and hardware qualification remain outstanding.
