# IndianOS P0 batch27 — 25 kernel-first source safety tasks, six validation workstations

**Repository:** pramod07info/indianos  
**Existing draft PR:** #788  
**Baseline:** 3093fda8001479b73fee4ab438229ba8e3bf1d36  
**Status:** Rust production changes and 25 named source-level regression tests added; all exact-head CI acceptance is pending.

| ID | Workstation | Component | Implemented acceptance test | Evidence status |
|---|---|---|---|---|
| P0-676 | W1 | PMM | Checked frame translation for first span page | Source added; exact-head CI pending |
| P0-677 | W1 | PMM | Span-offset and end-bound ownership | Source added; exact-head CI pending |
| P0-678 | W1 | PMM | Physical address overflow denial | Source added; exact-head CI pending |
| P0-679 | W1 | PMM | Corrupt span count and multiplication denial | Source added; exact-head CI pending |
| P0-680 | W2 | APIC timer | First delivered IRQ authorizes capture | Source added; exact-head CI pending |
| P0-681 | W2 | APIC timer | Capture count never exceeds delivered IRQs | Source added; exact-head CI pending |
| P0-682 | W2 | APIC timer | Zero IRQs never manufacture a preemption | Source added; exact-head CI pending |
| P0-683 | W2 | APIC timer | Stopped timer and saturated counters deny capture | Source added; exact-head CI pending |
| P0-684 | W3 | Ring3 preemption | CPU-generated RF hardware IRQ frame admitted | Source added; exact-head CI pending |
| P0-685 | W3 | Ring3 preemption | Clear only RF; preserve IF and saved registers | Source added; exact-head CI pending |
| P0-686 | W3 | Ring3 preemption | Reject RF injection through syscall capture | Source added; exact-head CI pending |
| P0-687 | W3 | Ring3 preemption | Reject TF, DF, AC and IOPL after RF cleanup | Source added; exact-head CI pending |
| P0-688 | W3 | Ring3 preemption | Failed IRQ capture cannot corrupt saved context | Source added; exact-head CI pending |
| P0-689 | W4 | Block I/O | Stable two-sector read remains functional | Source added; exact-head CI pending |
| P0-690 | W4 | Block I/O | Media replacement prevents partial data publication | Source added; exact-head CI pending |
| P0-691 | W4 | Block I/O | Second-sector I/O failure scrubs earlier bytes | Source added; exact-head CI pending |
| P0-692 | W4 | Block I/O | Out-of-range multi-sector write causes no mutation | Source added; exact-head CI pending |
| P0-693 | W5 | Virtio Ethernet | Legitimate IPv4 and ARP ingress retained | Source added; exact-head CI pending |
| P0-694 | W5 | Virtio Ethernet | Foreign unicast rejected before exposure | Source added; exact-head CI pending |
| P0-695 | W5 | Virtio Ethernet | Forged multicast and zero source rejected | Source added; exact-head CI pending |
| P0-696 | W5 | Virtio Ethernet | VLAN and 802.3 length bypass refused | Source added; exact-head CI pending |
| P0-697 | W6 | Limine boot | Two disjoint usable regions validated | Source added; exact-head CI pending |
| P0-698 | W6 | Limine boot | Reserved-region overlap refuses PMM handoff | Source added; exact-head CI pending |
| P0-699 | W6 | Limine boot | Malformed reserved memory-map tail rejected | Source added; exact-head CI pending |
| P0-700 | W6 | Limine boot | Unaddressable high RAM denied | Source added; exact-head CI pending |

## Production code changes
- PMM: checked span lookup and checked physical offset arithmetic before frame translation; corrupt span metadata cannot panic or wrap.
- APIC: real IRQ receipts now bound the number of saved ring-3 preemption captures; stopped timers cannot mint them.
- Ring-3 scheduler: normalize only the CPU-supplied RF flag on an actual timer capture before existing strict IRET checks; raw syscall frames and other dangerous flags remain rejected. This directly targets exact-head R024 diagnostics reporting 250 ring-3 IRQs, 250 rejected frames, RIP 0x400005, CS 0x2b, RFLAGS 0x10206.
- Storage, Ethernet, Limine: additional executable fail-closed regression coverage on real production validators, not claimed physical hardware enablement.

## Acceptance policy
- Six dedicated CI matrix lanes, **4 + 4 + 5 + 4 + 4 + 4 = 25 tests**, each with exact-commit checkout and no_std x86-64 cargo check.
- Full kernel boot **must** pass `make test-preempt` and emit genuine PID1 and PID2 APIC_TIMER_PREEMPTED markers, plus the R024 APIC round-robin PASSED marker. Merely counting IRQ EOI is insufficient.
- Keep PR #788 **draft**, do not merge until all required workflows and independent review pass on precisely the new SHA.
- Real SMP/IPI/TLB lifecycle, physical NIC IRQ/DMA/IOMMU, destructive disk power-loss recovery, live ring-3 PTE authorization and TPM verified boot remain **OPEN**.
