# PR #782: next ten coordinated kernel safety workstreams (WS11-WS20)

This batch builds on head `62b2b3bcb1f090efa1e70fc95fb5e7bb22f9f9ac` without modifying or overwriting prior commits.
It is a single additional source integration commit, not a record of ten
independent agent processes.

| Track | Production code change | Regression |
| --- | --- | --- |
| WS11 TinyFS | Validate complete persisted directory before admitting mount | malformed size, duplicate name, invalid path metadata |
| WS12 Capabilities | Selective owner+resource revocation preserving peer tickets | grant isolation and nonreuse |
| WS13 VFS | Transactional canonical basename copying for Files/Terminal | traversal, root, capacity, no partial output |
| WS14 Process events | Atomic ordered snapshots with one-time receipt IDs | ring wrap, replay, capacity |
| WS15 BSP lifecycle | Exact live permit identity check on retirement gate | foreign same-CPU permit denied |
| WS16 NIC RX | Bounded BSP-only software drain probe before rebind | busy, drained, stale receipt |
| WS17 SMP lifecycle | Explicit exact-generation AP ACK reclamation gate | pending, stale, wrong CPU |
| WS18 PCI topology | Reject malformed partially disabled bridge windows | impossible secondary/subordinate registers |
| WS19 Filesystem guard | Deny ambiguous compatibility backslash in guarded paths | protected namespace aliases |
| WS20 Live UDP ingress | Capture revision only after exact BSP + NIC permit checks | foreign gate, foreign NIC and quiescing ring |

## Boundaries (remain open)

The sources remain host-testable. The actual native disk power-loss recovery,
physical NIC IRQ/DMA/IOMMU ownership, complete SMP scheduler and TLB shootdown,
verified boot root, desktop launch and end-to-end persistent FS acceptance are
not established by these functions alone.

## Required exact-head evidence

- [ ] Full host Cargo suite; no_std kernel compilation, Rust formatting, security suites
- [ ] Genuine QEMU kernel/desktop boot, keyboard/mouse and disposable-disk cold reboot validation
- [ ] Two-vCPU AP admission and lifecycle validation, protected branch review
- [ ] Main commit verification after merging; no high-level gate checked without evidence
