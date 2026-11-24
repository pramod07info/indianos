# G5 input IRQ return regression evidence

The production input entry in `rust/kernel_core/src/arch/x86_64/keyboard_entry.S`
already saves all 15 general registers, clears DF before calling Rust, and clears
CPU-generated RF in a user return frame. The merged implementation is reused;
this change adds executable regression coverage rather than another IRQ path.

Run `python3 -B -m unittest discover -s tests -p 'test_input_irq_return.py' -v`.
The test requires GCC, compiles the actual production assembly with mouse IRQs
enabled, and substitutes a host capture trampoline only for the privileged
`iretq` instruction. It executes both production entry points with synthetic
kernel and user frames, five flag patterns including RF, IF and DF, and unique
sentinels in all 15 saved registers. The dispatcher deliberately clobbers those
registers. The oracle verifies register restoration, dispatcher stack alignment,
DF clear at dispatch, unchanged RIP/CS, and exact stacked flags: only user RF may
be removed. Twenty entry/frame combinations execute in the positive test.

Three independently compiled mutation controls must fail the same oracle:
removing RF normalization, removing the supervisor-frame guard, and restoring
RAX into RCX. This checks that a green test depends on the production behavior.
Compilation failures, missing GCC and execution timeouts fail the test instead
of silently skipping it. The normal-desktop mandatory workflow runs this suite
and retains its output in `input-irq-return.log`; the full Python suite also
discovers it.

This host transport does not execute an actual hardware interrupt, privileged
IRET, PIC/APIC acknowledgement, or privilege transition. It cannot establish
physical keyboard/mouse support or accept G5 by itself. Existing final-head
QEMU desktop tests remain responsible for actual IRQ/IRET, input routing,
window focus/capture and graphical acceptance. No new QEMU result is claimed
by this document.
