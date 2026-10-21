# M008 — Process control blocks and sequential user execution

**Verified in QEMU/OVMF on 2026-10-21.** IndianOS executes two different
freestanding x86-64 ELF64 images with independently staged user mappings,
distinct PIDs and complete frame/table reclamation between launches. It is
still a research kernel, **not** a preemptive scheduler or a secure
multi-user operating system.

## Process lifecycle implemented

A new, freestanding process table supports at most four records. PIDs start at
1 and increase monotonically within the initialized table. Slots are not
recycled while the table is live; exhausted capacity and invalid transitions
fail closed.

```text
            +-------+
            |  NEW  |           (create and assign PID)
            +---+---+
                |
                v
            +-------+
            | READY |           (ELF loaded in detached VMM)
            +---+---+
                |
                v
            +---------+
            | RUNNING |         (process-owned CR3 active in ring 3)
            +----+----+
                 |
        +--------+--------+
        |                 |
        v                 v
    +--------+        +---------+
    | EXITED |        | FAULTED |
    +---+----+        +----+----+
        |                  |
        +--------+---------+
                 |
           resources_reclaimed=true
           address_space_root=0
           state remains EXITED/FAULTED
```

The PCB stores PID, lifecycle state, page-table root while running, exit
status, fault vector, fault error/address and a resource-reclamation bit.
Invalid state transitions, duplicate completion and duplicate reaping are
rejected. A process can be considered a successful completion only when it
exited with code 0 and its memory has been released.

The ring-3 runner now accepts a PCB and performs the transitions around
image loading, CR3 activation, syscall/exception return and verified cleanup.
Historical M006/M007 single-process test commands use a one-record table and
still work.

## Two independent programs

- `/bin/init` is built from `userland/demo/ring3_entry.S` and prints
  **Namaste from IndianOS user mode!**.
- `/bin/worker` is built separately from
  `userland/demo/worker_entry.S` and prints
  **IndianOS worker process 2 completed**.
- The deterministic read-only initramfs holds both distinct ELF files when
  `PROCESS_SELFTEST=1` is selected. Normal boots still contain only the inert
  initial fixture and do not execute user applications.
- The first user image is allocated, protected, entered, exited and **fully
  torn down before** the second is staged. Both go through the same kernel
  ELF validation, PMM, VMM, IDT, TSS and syscall APIs, but their page-table
  lifetimes do not overlap.

## QEMU test and evidence

Verified [GitHub Actions run 39532527694](https://github.com/pramod07info/indianos/actions/runs/39532527694):
the `indianos/processes-qemu` commit status was `success`, with the entire
job successful. Host process lifecycle tests, existing single-process ring-3,
user-fault isolation, normal boot and deliberate kernel exception checks also
passed in that run.

Selected verified serial log:

```text
IndianOS: M008 sequential process launch begin
IndianOS: PID 1 READY
IndianOS: PID 1 RUNNING
IndianOS user: Namaste from IndianOS user mode!
IndianOS: PID 1 EXITED
IndianOS: ring-3 PMM frame count restored
IndianOS: PID 1 REAPED
IndianOS: PID 2 READY
IndianOS: PID 2 RUNNING
IndianOS user: IndianOS worker process 2 completed
IndianOS: PID 2 EXITED
IndianOS: ring-3 PMM frame count restored
IndianOS: PID 2 REAPED
IndianOS: M008 two independent processes exited and reaped
IndianOS QEMU boot smoke test PASSED
```

The smoke checker requires **exactly two** ring-3 entry markers and two
frame-count-restoration markers. It rejects a faulted task, a premature second
launch, missing process events or an `IndianOS: FATAL` message. The tests
also validate that the initramfs contains two separate, valid ELF64 images.

## Reproduction

On Linux/WSL with Clang, LLD, Make, Python 3, QEMU x86-64, OVMF, xorriso
and the pinned Limine bootloader:

```sh
make test-host
make test-processes
```

`make test-processes` creates `build/processes/` for this test variant,
validates both archive entries and checks that QEMU emits the entire two-task
lifecycle. The normal user mode and isolation tests remain independent:

```sh
make test-usermode
make test-userfault-supervisor
make test-userfault-guard
```

The GitHub Actions workflow now includes an enforced M008 QEMU regression
gate. Tests run only against the emulator image, never against physical disks.

## Limits and next priority

M008 is **sequential cooperative launching** with interrupts disabled. The
two userspace contexts are not alive concurrently; there is no scheduling
queue, timeslice, timer, task switch, IPC, filesystem API, fork, or process
parenthood. It is not yet capable of general-purpose multitasking and the
experimental memory isolation is not a security-audited sandbox.

**M009 candidate:** introduce a scheduler-independent trap frame and
per-process kernel stacks, then an interrupt-safe tick source and
round-robin scheduling under QEMU. Add tests for fair switching, faulting
one process without terminating another, per-process user-page ownership and
proper TLB handling before attempting SMP or real hardware.
