# M009 — Round-robin user contexts and x86 timer experiment

**Implemented and QEMU-verified:** two independently mapped ring-3 programs
alternate their CPU execution by voluntarily invoking `SYS_YIELD (3)`. Each
program is resumed at the exact instruction after its syscall with saved GPRs,
user stack and independently retained CR3. Both exit safely, their PCB roots
are reaped, and the test passes in QEMU/OVMF.

**Hardware timer status:** an additional, separately gated 8259 PIC / 8254 PIT
IRQ0 experiment has been implemented but **its real preemptive switch must not
be considered verified until the `indianos/preempt-qemu` commit status
passes**. Earlier QEMU runs reported zero delivered IRQ0 interrupts despite
user `RFLAGS.IF=1`. A legacy-PC routing/HPET experiment remains under test.
No normal boot or required CI gate enables this experimental IRQ mode.

## Implemented scheduling primitives

- **Process Control Blocks:** the M008 monotonic PID and lifecycle states
  extend to allow RUNNING → READY on yield and READY → RUNNING on resume.
  A task retains its CR3 root while paused. Bad roots, invalid state
  transitions, double reaping and foreign-PCB yield attempts are rejected.
- **Round-robin policy:** bounded ready-queue scan, fair cursor advancement,
  logical tick accounting, dispatch/yield counters, and refusal to run
  completed/reaped tasks. `INDIANOS_SCHEDULER_MAX_DISPATCHES` is 32 for the
  deterministic cooperative test; preemption variants use a larger, still
  bounded budget.
- **Per-process kernel stacks:** a separate 32 KiB TSS.RSP0 trap stack for
  each of the two tasks; the active stack is selected before every user-mode
  entry or resume. The existing IST double-fault emergency stack remains
  separate. These stacks are not yet protected with guard pages.
- **Saved CPU frame:** a software interrupt captures fifteen general-purpose
  registers and the five machine-generated IRET frame values (RIP, CS,
  RFLAGS, RSP, SS). The kernel owns this saved state, not the user process.
  `x86_64_resume_user` changes CR3 and restores that state using IRETQ;
  `x86_64_user_exit_to_kernel` first returns to the original kernel CR3
  and calling stack. The `SYS_YIELD` result is zero on resume.
- **Address-space ownership:** two program images at the *same virtual
  address* map to separate physical user frames and different CR3 roots.
  The upper supervisor mappings remain kernel-only. An idle task's physical
  user pages and page-table root remain allocated until it exits or faults.
  Reaping occurs only after the original bootloader CR3 is active.
- **Syscall ABI:** provisional syscall number 3 requests cooperative yield,
  accepting no arguments. Existing WRITE, EXIT and ABI_QUERY behavior is
  unchanged.

## Verified QEMU cooperative test

Use:

```sh
make test-host
make test-scheduler
```

These commands require a Linux/WSL development host with Clang, LLD, QEMU
x86-64, OVMF, xorriso, Python 3 and the pinned Limine release. The QEMU
scheduler fixture uses two distinct ELF images:

- `userland/demo/scheduler_a.S`
- `userland/demo/scheduler_b.S`

Both enter ring 3 with interrupts disabled, print a numbered message,
`SYS_YIELD` twice, resume after the syscall, and finally `SYS_EXIT(0)`.
The verified ordering is:

```text
IndianOS user: M009 task A step 1
IndianOS user: M009 task B step 1
IndianOS user: M009 task A step 2
IndianOS user: M009 task B step 2
IndianOS user: M009 task A step 3
IndianOS user: M009 task B step 3
IndianOS: M009 round-robin cooperative switching PASSED
IndianOS: M009 scheduler experiment complete
```

The strict smoke verifier also requires three DISPATCHED events and two
YIELDED events per PID, plus successful final reaping. This is confirmed by
[GitHub Actions run 39537052726](https://github.com/pramod07info/indianos/actions/runs/39537052726),
including its uploaded M009 scheduler transcript. Its QEMU job and the host
tests passed. A required CI regression gate now enforces cooperative
round-robin success on future code changes.

## Opt-in PIT IRQ0 preemption test — not yet complete

`kernel/arch/x86_64/timer.c` and
`kernel/arch/x86_64/user_entry.S` include optional PIC remapping to IRQ0
vector 0x20, a PIT ~100 Hz channel 0, DPL0 IRQ entry, CPU trap frame
capture and restoration, and interrupt acknowledgement. A separate image
build enables `RFLAGS.IF=1` for user code, with two CPU-bound applications
that do not issue `YIELD`:

```sh
make test-preempt
```

This test is isolated from the working cooperative kernel, uses a legacy
virtual machine configuration and is **non-blocking** in CI until verified.
Its dedicated commit status `indianos/preempt-qemu` is the authority for
success. Earlier attempts completed both CPU-bound programs without any
timer IRQ; extending the workload and probing PIC/IMCR/HPET delivery are
under investigation. A successful emulator boot **by itself** is not proof
of hardware-timer task preemption.

## Safety limitations / next steps

M009 runs a maximum of two tasks on one CPU under QEMU. The cooperative mode
depends on applications voluntarily yielding; it does **not** interrupt a
non-cooperative task. Work still needed: verify hardware timer routing and
preemptive switching; correct TLB/PCID and APIC behavior on other firmware;
add guard pages and overflow handling for privileged interrupt stacks; audit
nesting, unexpected interrupts, IRQ/EIO paths and task faults; design process
termination and starvation policies; implement synchronization, IPC and SMP
only after safe timer and context-switch behavior is independently proven.

This is a research OS kernel, not a production-ready, installable, secure
desktop OS. Do not test it on physical computers or real disks.
