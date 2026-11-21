# Batch 30 — P0-746–765: kernel-first implementations across six CI workstations

**Integration:** append to existing draft PR #788, based on exact head `0591a0471e7d6a9142b723af988d32ddf51173b2`. Keep one consolidated source commit for this batch. The six workstations are **separate CI test jobs, not six spawned coding agents**.

## Task ledger (implementation and named regression contract)

| Workstation | ID | Implementation and check |
|---|---|---|
| W1 PMM | 746 | Exact-set claim validates the complete bitmap/span geometry before mutation; successful discontiguous roundtrip test |
| W1 PMM | 747 | Reject overlapping physical spans without partially allocating |
| W1 PMM | 748 | Exact-set release rejects corrupt span offsets; retains live bitmap owners |
| W1 PMM | 749 | Bitmap accounting and usage snapshots reject malformed cursor/geometry before traversal/publication |
| W2 APIC | 750 | Keep valid bounded delivered IRQ/preemption receipts |
| W2 APIC | 751 | Deny saturated IRQ delivery receipts rather than wrap into a new capture budget |
| W2 APIC | 752 | Deny saturated/stale timer epochs and counter rewind |
| W3 Ring3 | 753 | Admit RIP inside actual executable ELF memory bytes |
| W3 Ring3 | 754 | Deny instruction pointer in executable page prefix before the loaded ELF bytes |
| W3 Ring3 | 755 | Deny instruction pointer in executable page suffix beyond the ELF memory size |
| W3 Ring3 | 756 | Apply loaded-byte bounds to actual numeric IRET frame validation |
| W4 Storage | 757 | Revision-pinned metadata API refuses media without a generation token |
| W4 Storage | 758 | Two-pass stable sector reads publish bytes with the exact generation receipt |
| W4 Storage | 759 | Generation change during read prevents stale bytes reaching caller |
| W5 Virtio | 760 | Preserve R028 IPv4 multicast and ARP/broadcast delivery |
| W5 Virtio | 761 | Reject arbitrary multicast MACs unsupported by the R028 Ethernet link |
| W5 Virtio | 762 | Deny invalid IPv4 multicast high bit and foreign unicast |
| W6 Boot | 763 | Preserve complete aligned canonical Limine descriptor admission |
| W6 Boot | 764 | Reject descriptor straddling the 48-bit canonical address hole |
| W6 Boot | 765 | Reject descriptor overflow, null and misaligned pointers |

## Acceptance policy

- `p0-batch30-implemented`: exactly **4+3+4+3+3+3 = 20** named Rust tests. Six `ubuntu-24.04` jobs require exact PR SHA and freestanding `x86_64-unknown-none` compilation.
- Existing Batch23–29 CI, QEMU boot, R024 true ring-3 preemption, persistent VFS, R028 network, security, release and kernel CI remain enabled.
- Source-level validations do **not** establish real SMP/IPI/TLB shootdown, physical NIC IRQ/DMA/IOMMU, power-loss journal recovery, trusted UEFI/TPM, live page-table protections, real hardware qualification, or independent protected-branch review.
- **Do not mark acceptance verified or merge** until all mandatory exact-head GitHub checks pass and the independent review gates are satisfied.
