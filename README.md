# IndianOS 🇮🇳

**IndianOS** is an experimental, independent desktop operating system for x86-64 PCs, designed for approachable everyday use in India. The goal is a familiar desktop workflow, Indian-language accessibility, and a native kernel—not a Linux distribution or a copy of Microsoft Windows.

> **Status (2026-10-21): M009 cooperative round-robin multitasking verified in QEMU.** IndianOS boots under QEMU/OVMF and now interleaves execution of two distinct ring-3 ELF64 applications using a new `SYS_YIELD` syscall. It saves and restores their CPU registers, stacks and distinct CR3 roots, retaining memory until each task exits. The earlier supervisor/guard fault-containment checks also pass. **Timer preemption is not yet verified**; this is not a production OS, audited sandbox or real-PC installer. Do not flash it to physical storage.

## Initial technical decisions

- **Target:** x86-64, UEFI-first; QEMU is the only supported test platform initially.
- **Kernel:** **Rust-first** `no_std` static library, with Rust-owned boot handoff, framebuffer, GDT/IDT/TSS, timer, main entry, memory management and scheduler selected by default. Some ring-3 CPU dispatcher and architecture paths still use freestanding C11/assembly. The Rust-default migration passed QEMU/OVMF CI on 2026-10-21.
- **Early boot:** use the open-source **Limine bootloader** and its published protocol as a temporary, well-defined handoff layer. The bootloader is *not* the kernel.
- **Graphics:** first draw pixels to the firmware-provided framebuffer; build a user-space compositor and desktop later.
- **Drivers:** bring up serial, framebuffer and timer first; then PCI, storage, keyboard and network devices.
- **Compatibility:** long-term native, web, Linux, Windows, Android and authorized remote-Mac routes; **none runs on IndianOS today**. Our independent kernel does not automatically run other platforms' binaries.
- **AI:** planned assistant-driven app discovery and secure installation via deterministic, policy-gated package services. No AI installer exists yet.
- **Portability:** x86-64 first; ARM64 requires a separate kernel/driver port. We cannot guarantee 100% compatibility or superiority to other operating systems.

## Read the design

1. [Architecture and boot-to-desktop flow](docs/ARCHITECTURE.md)
2. [Phased development roadmap](docs/ROADMAP.md)
3. [Architecture decision record](docs/ADR-0001-foundation.md)
4. [Boot contract and error handling](docs/BOOT_CONTRACT.md)
5. [Driver bring-up and hardware strategy](docs/DRIVER_STRATEGY.md)
6. [Desktop and India-first UX plan](docs/DESKTOP.md)
7. [Milestone 001: first kernel boot](docs/MILESTONE_001.md)
8. [Milestone 002: exceptions and physical memory](docs/MILESTONE_002_IMPLEMENTATION.md) and [M001 boot guide](docs/BOOT_M001_IMPLEMENTATION.md)
9. [Milestone 003: detached virtual memory](docs/MILESTONE_003_IMPLEMENTATION.md)
10. [Milestone 004: paging preflight, TSS/IST and page faults](docs/MILESTONE_004_IMPLEMENTATION.md)
11. [Milestone 005: ELF64 user program and syscall ABI foundation](docs/MILESTONE_005_IMPLEMENTATION.md)
12. [Cross-platform app compatibility plan](docs/COMPATIBILITY.md)
13. [AI app assistant and secure install architecture](docs/AI_APP_ASSISTANT.md)
14. [ADR: compatibility and AI](docs/ADR-0002-interoperability-ai.md)
15. [Performance and portability targets](docs/PERFORMANCE_GOALS.md)
16. [Milestone 007: QEMU-verified recoverable user faults](docs/MILESTONE_007_USER_FAULTS.md)
17. [Milestone 008: PCB lifecycle and two independent user ELF processes](docs/MILESTONE_008_PROCESSES.md)
18. [Milestone 009: round-robin contexts and timer experiment](docs/MILESTONE_009_SCHEDULER.md)
19. [Rust-first kernel rewrite (tracked migration)](docs/RUST_MIGRATION.md)

## Directory plan

```text
boot/          Boot protocol adapter and future boot experiments
kernel/        Architecture, memory, interrupts, tasks, syscalls
drivers/       Device discovery and hardware drivers
userland/      Init, core libraries, services and applications
desktop/       Compositor, shell and desktop applications
tools/         Emulator, image-building and test utilities
docs/          Decisions, interfaces, roadmap, acceptance criteria
```

The boot and kernel directories now include the early kernel code; the desktop, drivers and userspace directories remain conceptual placeholders.

## Offline app compatibility planner (development prototype)

The sample manifest contains fictional applications and planned provider routes. It is **not** a running IndianOS app store, AI model, installer, or binary-compatibility layer. On a development machine with Python 3.10+:

    python3 tools/app_resolution.py --catalog examples/app_catalog.json --device examples/device_profile.json --app "Sample Notes"
    python3 -m unittest discover -s tests -v

All execution paths are informational; no apps are downloaded or installed. The companion design and security model are documented in [AI_APP_ASSISTANT.md](docs/AI_APP_ASSISTANT.md).

## First kernel implementation (M001)

The code now contains a standalone x86-64 kernel entry, COM1 serial diagnostics, Limine protocol base revision and memory-map validation, an optional bounded tricolour framebuffer display, and safe halt behaviour. **A compiled ELF is not proof of a completed QEMU boot.**

For development on Linux/WSL2, install `clang`, `lld`, `binutils`, `make`, `curl`, `xorriso`, `qemu-system-x86`, `ovmf` and Python 3:

```sh
make toolchain-check     # verify compiler toolchain
make kernel              # compile freestanding ELF (offline)
make image               # fetch SHA256-pinned Limine binary and build ISO
make test-boot           # run UEFI QEMU smoke test against serial milestones
make run                 # interactive UEFI QEMU boot
python3 -m unittest discover -s tests -v
```

The test harness detects ordered serial markers and terminates QEMU after success; the kernel itself halts rather than exiting. Pass `OVMF_CODE=/path/to/OVMF_CODE_4M.fd` if firmware is installed elsewhere. All generated files are beneath `build/`. See [implementation details](docs/BOOT_M001_IMPLEMENTATION.md).

## Kernel foundations (M002)

The x86-64 kernel now has a minimal own GDT and IDT, error-aware assembly exception stubs, and a conservative bitmap allocator for usable physical memory frames. An intentional `UD2` test build exercises exception vector 6. Physical frames are **not** automatically mapped or used.

```sh
make kernel          # native x86-64 ELF compile
make test-host       # PMM C harness + Python serial transcript tests
make test-boot       # normal Limine/QEMU/OVMF boot (requires emulator)
make test-exception  # separate deliberate-fault QEMU test
```

The normal and exception variants compile locally, and host unit tests pass. Their actual execution in QEMU remains to be verified. See [M002 implementation and limitations](docs/MILESTONE_002_IMPLEMENTATION.md).

## Virtual memory foundations (M003)

Added a software-controlled x86-64, four-level page-table manager with mapping,
query, unmapping, permission validation, failure rollback and page-table-frame
reclamation. Its kernel smoke check uses the Limine HHDM to access PMM-backed
page-table frames and validates a **detached** mapping. It deliberately does
**not** switch the active CR3 root or implement usable user processes.

```sh
make kernel
make test-host       # includes VMM C tests and 8 serial parser tests
make test-boot       # requires QEMU/OVMF/xorriso, not run locally
```

See [M003 implementation and security boundaries](docs/MILESTONE_003_IMPLEMENTATION.md).

## CPU paging protections and exception diagnostics (M004)

Added CPU paging capability discovery and strict switching preflight (NX/NXE, WP, PAE and four-level checks), a dedicated TSS/IST1 double-fault emergency stack, and detailed page-fault reporting with CR2. A bounded page-table-cloning engine has host tests. **The default kernel never changes CR3**; an opt-in QEMU-only variant can attempt a guarded clone/switch/restore round trip, pending runtime verification.

On Linux/WSL:

    make kernel
    make test-host
    make test-pagefault   # needs QEMU, OVMF and xorriso
    make test-shadow      # experimental, manual QEMU-only test

See [Milestone 004 implementation and limitations](docs/MILESTONE_004_IMPLEMENTATION.md). This is not yet a process-isolated or installable OS.

## First user-program foundation (M005, initial slice)

Added a bounded, strict ELF64/x86-64 static executable layout validator,
a **planned** 64 KiB user stack with an **unmapped guard-page reservation**,
and a provisional syscall ABI validator for ABI_QUERY, WRITE and EXIT.
The repository additionally builds a real, freestanding ELF64 demo fixture
and validates it from a host program. It does not execute the demo.
The kernel's boot-time self-test uses a synthetic ELF image: **it does not
load, map, enter ring 3 or execute any application**.

```sh
make kernel       # freestanding kernel compile
make test-host    # includes ELF64 and syscall ABI host tests
make test-user-elf # build a real freestanding ELF and validate its layout
make test-boot    # requires QEMU/OVMF/xorriso for runtime smoke checks
```

See [Milestone 005 implementation and remaining isolation requirements](docs/MILESTONE_005_IMPLEMENTATION.md).

## Read-only initramfs and user-page staging (M005 continuation)

The QEMU ISO now packages a deterministic read-only archive containing a freestanding
`/bin/init` ELF. Limine advertises the archive as a module. During kernel startup,
IndianOS validates the archive, allocates and zero-fills physical pages, copies
program bytes and builds an isolated-looking **detached page-table tree**, with
a writable non-executable stack and unmapped guard page. **It does not load
this tree into CR3 or execute the program in ring 3.** All transient physical
frames and tables are reclaimed after the boot self-test.

```sh
make test-host       # archive parser, user image mapping and failure rollback
make test-initramfs  # compile /bin/init, pack archive, validate binary
make image           # QEMU-only ISO with Limine initramfs module
make test-boot       # QEMU/OVMF required, not yet verified locally
```

See [M005 initramfs and user-image staging](docs/MILESTONE_005_INITRAMFS.md).

## First native IndianOS user application — verified in QEMU (M006)

**Verified 2026-10-21:** the syscall-enabled `/bin/init` executed in CPU
ring 3 under QEMU/OVMF, printed **Namaste from IndianOS user mode!** through
`int 0x80`, exited successfully, returned to the kernel, released its
supervisor clone and user memory, and restored the PMM free-frame count.
The complete GitHub Actions job passed on
[verified code commit `4fac1ad`](https://github.com/pramod07info/indianos/commit/4fac1ad4d2e50ea5d837b1d9968ec33648066fd2).

```sh
make test-host
make test-ring3-elf
make test-usermode     # QEMU + OVMF + xorriso + pinned Limine required
```

The regular boot ISO still uses the inert demo; `USERMODE_SELFTEST=1` is
required to execute the test-only `userland/demo/ring3_entry.S`. The normal
hardware and security limitations remain: this is **one experimental user
process**, not a scheduler, general application runtime, desktop, or a
production-ready operating system.

See [verified serial output and debugging history](docs/RING3_VERIFIED.md)
and [M006 architecture](docs/MILESTONE_006_USERMODE.md).

## Ring-3 fault isolation — verified in QEMU (M007)

IndianOS now terminates the active experimental user task instead of halting
the entire kernel when it attempts to read supervisor-only text or an unmapped
user-stack guard page. QEMU verifies #PF origin and CR2, restoration of
the original CR3, safe user table detachment and no leaked PMM frames.
The normal and deliberately fatal **kernel-origin** exception tests remain
fatal and continue passing.

```sh
make test-host
make test-usermode
make test-userfault-supervisor
make test-userfault-guard
```

[Passing QEMU run 39531137405](https://github.com/pramod07info/indianos/actions/runs/39531137405)
covers the first two user fault probes and the original `int 0x80` user app.
See [M007 recovery tests and limitations](docs/MILESTONE_007_USER_FAULTS.md).

This is one sequential test process with hardware interrupts off; timers,
multi-process scheduling and release-grade application isolation do not exist.

## M008 — Two sequential native processes in QEMU

IndianOS now has a bounded PID/PCB table and explicit user task lifecycle.
The process test packs two **different** ELF images (`/bin/init` and
`/bin/worker`) into its read-only initramfs and runs them in CPU ring 3
one after the other. Each process prints through the IndianOS syscall gate,
exits and releases its page tables and data frames before the next starts.

```sh
make test-host       # includes PCB state/invalid-transition tests
make test-processes  # two separate QEMU user programs, PID and PMM assertions
```

[Verified dual-process CI run](https://github.com/pramod07info/indianos/actions/runs/39532527694)
and [M008 technical notes](docs/MILESTONE_008_PROCESSES.md) document the
success trace. The normal ISO still does not automatically run user programs;
the dual-process execution is an explicit QEMU-only test variant.
**This is sequential launching, not multitasking or CPU scheduling.**

## M009 — Cooperative round-robin multitasking verified in QEMU

Two independently mapped ring-3 applications now **alternate execution** in
IndianOS. Each has a distinct CR3 root, process control block, privileged
kernel interrupt stack, saved GPR/IRET frame and syscall-yield return state.
The test interleaves `A1, B1, A2, B2, A3, B3` with frame ownership retained
between yields and fully reclaimed after the final exit.

```sh
make test-host        # process/scheduler policy and invalid-transition tests
make test-scheduler   # verified CPU-context preservation across real yields
make test-preempt     # separate PIC/PIT hardware IRQ experiment, NOT verified
```

[QEMU evidence](https://github.com/pramod07info/indianos/actions/runs/39537052726)
and [M009 implementation](docs/MILESTONE_009_SCHEDULER.md) record the
successful context switches. **Cooperative multitasking is verified.**
Legacy IRQ0 **timer-driven preemption remains experimental**; ordinary boots
still keep hardware interrupts off, and CI does not claim IRQ0 success.

## Rust-first migration (partial, verified)

IndianOS links a **`no_std` Rust kernel core by default**. Earlier Rust
process, scheduling, fault, ELF, memory and syscall paths have QEMU evidence;
the newer Rust boot protocol, framebuffer, descriptor tables, timer link,
kernel entry, userland demo binaries and native host tooling are now selected
by default and passed [integrated QEMU/OVMF run 39558378245](https://github.com/pramod07info/indianos/actions/runs/39558378245)
on [code commit fc25814](https://github.com/pramod07info/indianos/commit/fc25814bcd42837c30eeb633c927b3f343be0267). The original C versions are retained for
rollback, but are excluded from the default freestanding ELF.

Requirements: stable `rustup`, `cargo`, `x86_64-unknown-none` target,
Clang/LLD and the existing QEMU/OVMF test dependencies.

```sh
rustup target add x86_64-unknown-none
make test-rust-host-abi   # Rust tests + unchanged C ABI/process/image tests
make kernel              # now Rust-backed by default
make test-rust-processes  # real user execution against Rust-backed kernel
make test-rust-scheduler  # real saved-context round-robin under QEMU
make test-rust-default-qemu # Rust-native Limine, IDT, kmain, apps and tools
make test-boot           # Rust-default normal ISO smoke under QEMU
# Emergency reference implementation while migrating:
make kernel BUILD=build/c-reference RUST_CORE=0
```

This is an **incremental rewrite, not yet a kernel written entirely in Rust**.
The remaining ring-3 execution machinery, some architecture-specific code and
reference test tooling still contain C/assembly/Python. Only a small amount
of architecture assembly should remain when Rust ownership is complete.
The migration is gated by original C API compatibility and actual QEMU
serial transcripts; see [Rust migration inventory and acceptance
criteria](docs/RUST_MIGRATION.md).

## Safety and legal

Never test early kernels on a primary machine or disk. Use QEMU disk images and snapshots. Do not copy proprietary Windows binaries, artwork, trademarks or source code. Third-party firmware, bootloader and toolchain components must retain their respective licenses. The IndianOS license has **not** yet been selected.

## Scope

This is a research prototype. Consumer usability, security, commercial support, an installer, full hardware support, an AI assistant, and real application compatibility require much later milestones and substantial engineering.

## GitHub contribution workflow

By project-owner preference, bounded development changes go **directly to main**. Pull requests are created only when the owner explicitly asks. Automated checks and release gates still apply.
