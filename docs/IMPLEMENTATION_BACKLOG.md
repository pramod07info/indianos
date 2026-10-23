# IndianOS actionable implementation backlog

Status captured 2026-10-23. This backlog tracks the path from an experimental
QEMU kernel to a usable OS. **An item is done only after its acceptance checks
are observed**. Source code, diagrams, and passing host tests alone do not
establish working interrupts, a desktop, or third-party application support.

## Verified foundations (not general-purpose production features)

- [x] UEFI/Limine x86-64 QEMU boot with a Rust-first no_std kernel
- [x] GDT/IDT/TSS, PMM/VMM, ring-3 fault isolation and basic syscalls
- [x] Two independent ring-3 programs using cooperative scheduling
- [x] Limited fixed-capacity IPC, capability, block, filesystem and
  interactive-console primitives with regression tests
- [x] Absolute VFS path normalization (R021)
- [x] R024 xAPIC timer preemption with genuine IRQ-to-context-switch proof in QEMU ([passing verification](https://github.com/pramod07info/indianos/actions/runs/39913743740))
- [x] Opt-in persistent QEMU serial shell with live worker launch, status, wait and cleanup ([passing verification](https://github.com/pramod07info/indianos/actions/runs/39924988234))
- [ ] Normal boot launching interactive userland by default and general service management

## Implementation sequence and exit conditions

| ID | Priority | Deliverable | Required exit evidence |
|---|---|---|---|
| R022 | P0 | Connect absolute VFS paths to TinyFs flat-root create/read/delete | Reject traversal and nested paths; preserve existing data; remount CRUD regression and no_std build |
| R023 | P0 | Bounded process-event FIFO with per-PID teardown | Zero/one-capacity, wraparound, overflow and ordering tests; no_std build |
| R024 | P0 | Calibrated local APIC preemption wired to scheduler | QEMU observes IRQs, capture, two CPU-bound tasks alternating without yield; memory/scheduler cleanup |
| R025 | P0 | **QEMU-verified** capability-gated process, IPC and flat-root file syscalls | **Verified:** ring-3 identity, rights, user-copy bounds, cross-PID denial, revocation and teardown; production service integration remains R026/R027 |
| R026 | P1 | **QEMU-verified** interactive init, restricted worker launch and lifecycle | **Verified:** serial QEMU commands, shell survives worker start/exit, process status/wait and cleanup. General service management remains future work |
| R027 | P1 | Device enumeration and reliable block/VFS stack | QEMU block tests, atomic metadata recovery, multi-block files, reboot durability |
| R028 | P1 | Virtio network driver, ARP/IP/ICMP/UDP/TCP/DNS | QEMU guest network integration, ping and bounded network-client demo |
| R029 | P1 | User-space compositor, mouse, windows and graphics | Two independent graphical windows interact in QEMU, not only headless models |
| R030 | P1 | Native toolchain SDK, apps and verified package manager | Repeatable signed install, update, remove and rollback of native packages |
| R031 | P2 | Security model, authentication, updater, recovery installer | Isolation review, controlled permission tests, signed rollback and virtual-disk install |
| R032 | P2 | Physical PC/ARM64 hardware-portability program | Published test matrix, qualified devices, measured boot/install performance |
| R033 | P2 | Policy-gated AI app assistant | AI chooses catalog item but package verifier enforces approval, signatures, compatibility and rollback |
| R034+ | P3 | Explicit Linux/Windows/Android and lawful macOS routes | Per-app ABI/runtime acceptance on tested configurations, no universal guarantee |

R022 and R023 are **kernel-library foundations**, not proof that filesystem
syscalls or scheduler lifecycle notifications are wired into live ring-3
execution. Those integrations belong to R025 and require new QEMU tests.

## Commit / validation policy

1. Work on an isolated verification branch to run host regression tests,
   `cargo build --no-default-features --target x86_64-unknown-none`,
   source audit, and targeted safety tests.
2. Commit only verified bounded source to `main`, fast-forward only, after
   checking the branch lease. Do not force-push or bypass validation.
3. Run full mandatory QEMU kernel CI on the final `main` SHA. Document CI
   separately from the branch test results; do not declare complete until it
   passes.
4. Never auto-format physical drives or enable experimental IRQs in ordinary
   boots. Preserve tested fallbacks and crash diagnostics.
5. Prioritize working integrations and observed user-visible behavior over
   creating isolated modules merely to increase commit counts.

A 100% Windows/Linux/macOS/Android binary-compatibility target is not
technically guaranteed; support must be measured app-by-app.

## R024 acceptance — Verified 2026-10-23

The R024 xAPIC verification job passed QEMU/OVMF and observed
`APIC_TIMER_PREEMPTED` for both CPU-bound ring-3 PIDs before either process
exited, scheduler resumption of both PIDs, eventual successful exits and
memory-frame reaping. Hardware APIC vector 0x40, supervisor UC MMIO,
PIT channel 2 timer calibration, and the spurious-vector handler are
isolated behind the opt-in preemption test. Legacy PIC IRQ0 no longer supplies
R024 scheduling. The main full kernel CI now treats the QEMU preemption
step as required, not a non-blocking experiment.

This verified test is specific to x86-64 QEMU/OVMF; SMP, real hardware,
secure interrupt-controller initialization for diverse firmware and
production-grade scheduler fairness remain unqualified.

## R025.1 — QEMU-verified secure IPC (2026-10-23)

The [R025 verification run](https://github.com/pramod07info/indianos/actions/runs/39916988928)
passed host policy tests, freestanding Rust compilation, source auditing,
and a real two-process QEMU ring-3 ping/reply. The sender ID came from
the active PCB and matching scheduler task. It rejected invalid recipient
PIDs and user pointers, preserved user output on empty queues, removed
mailboxes at process exit, and reaped both processes exactly once.

R025 **is not complete**: capability-mediated authorization, secure file
syscalls, process-service APIs and general blocking IPC still require
implementation and QEMU regression testing. The ordinary kernel boot
continues to keep this test-only IPC ABI disabled. R025.1 is now required
in full kernel CI, not merely a one-off branch verification.

## R025 completion acceptance (2026-10-23)

The end-to-end `test-r025-complete-qemu` verifies two independent
ring-3 processes, kernel-derived identities, constrained capability
grants, IPC message permissions and responses, process status queries,
per-process TinyFs namespaces, file create/read/delete syscalls,
foreign-object rejection, safe bounded user buffers, explicit token
revocation, and automatic capabilities/mailbox cleanup during process
exit. R025.1's original non-capability test continues to run separately.

The R025 full QEMU execution is mandatory in the kernel CI workflow.
Only the bounded experimental QEMU service is complete: multiuser,
persistent and crash-recoverable file services, blocking multiprocess
IPC, SMP and real-hardware security are outside this acceptance.

## R026 acceptance — Live interactive init (2026-10-23)

The [R026 QEMU verification](https://github.com/pramod07info/indianos/actions/runs/39924988234)
passed kernel process/syscall tests, Rust command-parser regressions,
freestanding no_std compilation, source audit and live QEMU serial scripting.
The shell remained alive through the exact sequence `help`, `ps`,
`run /bin/worker`, `ps`, `wait /bin/worker`, duplicate `run`,
and `exit`. The worker was not runnable before the authenticated launch.
After the command, the worker ran in its distinct ring-3 image, exited
and was reaped before PID1 resumed and accepted more commands.
The shell confirmed the child's state, rejected relaunch and shut down
only at the user's request. Both PIDs were reaped exactly once.

The ordinary default kernel does **not** start this experimental shell.
The service supports one allowlisted worker launch per boot; arbitrary
ELF execution, repeated process creation, persistent filesystem, job
control, background services and multiuser sessions remain outside
R026's *bounded QEMU* acceptance and must be developed later.

Full `main` kernel CI requires the R026 serial-interactive QEMU test.
