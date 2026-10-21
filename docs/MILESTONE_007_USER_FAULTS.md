# M007 — Recoverable ring-3 user faults (QEMU/OVMF)

**Status: verified in QEMU on 2026-10-21.** This is a narrowly scoped single-process fault-containment experiment; not a production scheduler or general-purpose kernel. Keep the test image off real hardware and real disks.

## What changed

The existing x86-64 IDT/GDT and test-only user-process runner now distinguish **ring-3 faults caused by the active user process** from fatal kernel faults:

- A pure, separately host-tested `indianos_user_fault_eligible` policy accepts only exception vectors **#UD (6), #GP (13) and #PF (14)** from the exact ring-3 code selector **0x2b** while a user task is active. It rejects kernel-origin exceptions, double faults, missing user contexts and page faults with a reserved-bit violation (which can signal corrupt paging structures).
- For eligible faults, the handler captures the vector, error code and CR2 (for #PF), then abandons the faulting user task through the existing kernel return trampoline. It **does not IRETQ back into the faulting instruction**.
- The return trampoline restores the original kernel CR3 and caller stack. The caller clears the active-process pointer, detaches borrowed user PML4 entries, frees cloned supervisor page tables, releases user pages and checks that the PMM free-frame count returns to its pre-launch value.
- Test-mode logic requires the **actual observed** page-fault vector and CR2 address plus the hardware error-code user/present bits; merely exiting through the `SYS_EXIT` syscall with a fabricated code does not count as successfully isolating a fault.

## Two live negative tests

Each QEMU test boots an independent IndianOS kernel image with one malicious `/bin/init` and finishes only after the kernel survives and reclaims its memory:

| Variant | Attempted access | Expected hardware fault |
| --- | --- | --- |
| `fault-supervisor` | Read `0xffffffff80000000`, a kernel-only code mapping | #PF, user=1, present=1 |
| `fault-guard` | Read `0x00007fffeffef000`, the first byte of the unmapped stack guard | #PF, user=1, present=0 |

A load that unexpectedly succeeds calls `SYS_EXIT(99)` and fails the check. The test runner requires these ordered serial markers, and rejects any `IndianOS: FATAL`:

```text
IndianOS: entering ring-3 /bin/init (QEMU experimental)
IndianOS: isolated ring-3 page fault
IndianOS: user page fault contained; kernel resumed
IndianOS: ring-3 borrowed user tables detached
IndianOS: ring-3 supervisor clone released
IndianOS: ring-3 user image released
IndianOS: ring-3 PMM frame count restored
IndianOS: user fault isolation experiment complete
IndianOS QEMU boot smoke test PASSED
```

The existing deliberate kernel-origin `UD2` and `#PF` tests remain **fatal as designed**, and still pass independently. The original successful `int 0x80` greeting/exit path also continues to pass.

## Verified evidence

[GitHub Actions run 39531137405](https://github.com/pramod07info/indianos/actions/runs/39531137405) completed with **all jobs successful**. Its published commit contexts include:

- `indianos/ring3-qemu = success`
- `indianos/userfault-qemu = success`

The raw job log contains both isolated user faults, both "kernel resumed" messages, both PMM restoration messages, all normal boot markers, and successful deliberate kernel-exception tests. Pure fault-origin policy and serial-parser regression tests also passed in CI. Subsequent pushes require the two negative QEMU tests as a regression gate.

## Reproduce

On Linux or WSL with clang, lld, make, xorriso, QEMU x86-64, OVMF, Python 3 and the pinned Limine release:

```sh
make test-host
make test-usermode
make test-userfault-supervisor
make test-userfault-guard
```

All QEMU build variants have separate `build/ring3`, `build/fault-supervisor` and `build/fault-guard` directories. The normal ISO remains free of experimental user execution.

## Scope and next steps

This mechanism supports **one synchronous process on one CPU, with maskable hardware interrupts disabled**. It is not a replacement for per-process kernel stacks, process tables, multi-process paging ownership, a scheduler, guard-paged kernel stacks, or secure context switching. Malicious code could exercise untested exception classes; kernel/driver/IO/firmware faults still halt the system. The code has not undergone a security audit.

**Next bounded milestone:** introduce a process control block/state machine (NEW/READY/RUNNING/EXITED/FAULTED), explicit exit/fault reasons and IDs, then run two independent user programs sequentially with isolated address spaces before introducing preemptive timers and context switches. User access to I/O ports, privileged instructions, and unexpected interrupt handling also require deliberate QEMU security tests.
