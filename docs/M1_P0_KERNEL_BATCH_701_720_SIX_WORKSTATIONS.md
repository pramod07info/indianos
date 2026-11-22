# IndianOS P0 batch28 — 20 kernel-first safety changes

**Repository:** pramod07info/indianos
**PR:** #788 (existing draft, no merge authorization)
**Baseline:** bdda98f7f3561878d6899cdfb968fcfb1165adf9
**Validation:** 6 independent GitHub Actions test lanes (4+3+4+3+3+3), with a real Rust test case for every item, plus per-lane freestanding no_std compilation.
**Acceptance status:** Source implementation staged. Exact-commit CI and independent review are pending. A prior CI run exposed an accidental preflight output regression; this revision restores the established preserved-buffer contract and updates W4 tests.

| Task | Lane | Production path and independent acceptance test |
|---|---|---|
| P0-701 | W1 / PMM | Allocations from disjoint physical regions retain unique page ownership |
| P0-702 | W1 / PMM | Overlapping firmware spans are denied before allocating a frame |
| P0-703 | W1 / PMM | Adjacent but non-overlapping spans remain usable |
| P0-704 | W1 / PMM | Reordered aliasing spans cannot mutate bitmap or cursor |
| P0-705 | W2 / APIC timer | Valid current epoch and delivered IRQ budget permit a capture |
| P0-706 | W2 / APIC timer | Stale stop/rearm session does not reuse previous capture authority |
| P0-707 | W2 / APIC timer | Unstarted, stopped, full-budget and zero-delivery sessions deny capture |
| P0-708 | W3 / ring-3 | Correct ELF file/memory bounds remain readable |
| P0-709 | W3 / ring-3 | File size greater than ELF memory size fails before user memory authorization |
| P0-710 | W3 / ring-3 | ELF virtual address below page ownership fails closed |
| P0-711 | W3 / ring-3 | ELF memory range crossing owned page end fails closed |
| P0-712 | W4 / storage | Invalid read output length preserves caller bytes and never reads device |
| P0-713 | W4 / storage | Out-of-capacity read preserves caller bytes and never reads device |
| P0-714 | W4 / storage | Valid sector read continues to return real device bytes |
| P0-715 | W5 / virtio-net | All-zero Ethernet destination is refused at ingress |
| P0-716 | W5 / virtio-net | Legitimate local unicast and broadcast ingress remains accepted |
| P0-717 | W5 / virtio-net | Foreign unicast remains excluded from guest stack |
| P0-718 | W6 / boot | Naturally aligned framebuffer response addresses are permitted |
| P0-719 | W6 / boot | Misaligned framebuffer descriptor pointers are denied |
| P0-720 | W6 / boot | Null response, pointer array and descriptor pointers are denied |

## Production changes
- PMM single-frame geometry rejects physical overlap even when bitmap offsets are consistent.
- Ring-3 timer preemption capture now checks session epoch before publishing evidence.
- ELF user segment validation bounds nonempty virtual memory payloads within owned pages.
- Block preflight preserves caller output on invalid length or capacity; mid-transfer failures still scrub output.
- Ethernet ingress rejects zero destination MACs while retaining discovery broadcast.
- Desktop framebuffer and desktop color helpers check Limine response/table/descriptor pointer alignment before constructing references.

## Integration and acceptance
1. Single batch28 source commit on the existing PR branch; inherited previous batches retained.
2. Dedicated six-lane batch28 CI checks require 20 named Rust regression tests and no_std x86-64 kernel compilation.
3. Keep the entire existing CI matrix enabled, including R024 genuine PID1/PID2 preemption, QEMU boot, durable storage, networking, security and kernel memory.
4. Do not merge without exact-head CI PASS and independent review. These tests are not physical SMP/IOMMU, destructive power-loss recovery, actual ring3 PTE enforcement or TPM verified-boot acceptance.
