# R024 — APIC preemptive scheduling proof

R024 must be verified by **two separate CPU-bound ring-3 processes** that
alternate without calling SYS_YIELD. The emulator must observe genuine
APIC vector 0x40 IRQs, capture both process contexts, and restore the
memory/frame count after both processes exit. A host unit test is not proof.

## Experimental implementation

- Install a dedicated supervisor-only, uncached 4 KiB xAPIC alias at
  `0xffff_fe00_0000_0000` **before** cloning user page tables.
- Check IA32_PAT entry 3 is UC and ensure the xAPIC physical base is valid.
- Enable xAPIC in the spurious-interrupt vector register and lower APIC task
  priority; do not disable xAPIC to route PIC IRQ0.
- Calibrate the masked APIC timer against PIT channel 2 without enabling a
  legacy PIC IRQ. Arm the calibrated periodic timer vector 0x40 at 100 Hz.
- Save real CPU GPR/IRET trap frames and require successful ring-3 context
  capture for the IRQ-to-scheduler preemption counter.
- Stop the timer and require both tasks to show timer preemptions and a
  positive, bounded IRQ and capture count.

The test is opt-in and QEMU-only; the regular kernel remains safe-halted
without initiating hardware scheduling.

## Acceptance

`make test-preempt` must pass, with a QEMU serial marker:
`IndianOS: R024 APIC timer-preemptive round-robin PASSED`.
The QEMU runner must reject failures, zero IRQs, missing captures, and
missing task alternation. Only after this confirmation may R024 be
consolidated into one verified commit on main.
