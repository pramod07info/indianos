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
- [ ] Hardware APIC timer preemption with genuine IRQ-to-context-switch proof
- [ ] Normal boot launching a persistent interactive userland service

## Implementation sequence and exit conditions

| ID | Priority | Deliverable | Required exit evidence |
|---|---|---|---|
| R022 | P0 | Connect absolute VFS paths to TinyFs flat-root create/read/delete | Reject traversal and nested paths; preserve existing data; remount CRUD regression and no_std build |
| R023 | P0 | Bounded process-event FIFO with per-PID teardown | Zero/one-capacity, wraparound, overflow and ordering tests; no_std build |
| R024 | P0 | Calibrated local APIC preemption wired to scheduler | QEMU observes IRQs, capture, two CPU-bound tasks alternating without yield; memory/scheduler cleanup |
| R025 | P0 | Authentication-gated process, IPC and file syscalls | Ring-3 QEMU tests enforce identity, capability rights, bad pointers, cleanup |
| R026 | P1 | Persistent interactive init, shell and process services | User can type commands and launch/reap programs via a live QEMU console |
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
