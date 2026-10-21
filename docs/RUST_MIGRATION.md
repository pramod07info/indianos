# IndianOS Rust-first migration — staged replacement (R001 onward)

**2026-10-21: RUST-FIRST DEFAULT, PARTIAL MIGRATION. Not yet a fully rewritten Rust operating system.**

The owner requested that all IndianOS-owned code be converted to Rust before
moving to another feature milestone. Converting a booting x86-64 kernel
all at once would remove the existing QEMU regressions and risk introducing
unreviewable assembly/ABI failures. This plan makes **Rust the destination
language** while requiring demonstrable kernel behavior after each component
replacement. Everything is committed directly to `main`, with no PR.

## Non-negotiable compatibility contracts

1. **Freestanding Rust:** `#![no_std]`, `panic=abort`, no allocator until
   one has been designed, no crates.io dependencies in early kernel crates,
   `x86_64-unknown-none` target. Stable Rust is preferred.
2. **No dynamic relocation:** the Rust kernel staticlib is compiled for
   `relocation-model=static` and `code-model=kernel`. Statically resolved
   GOT entries, if emitted by the compiler (e.g. calls to `memcpy`), are
   included in a read-only supervisor ELF segment; no runtime loader.
3. **Explicit FFI:** `#[repr(C)]`, exact enum/boolean widths, layouts,
   pointer lifetimes, ownership, checked arithmetic, and no unwinding over
   a C/assembly boundary. The original ABI headers and C test fixtures serve
   as the executable migration specification.
4. **Real OS verification:** compile and run Rust unit tests, the original C
   ABI tests linked against Rust, and QEMU tests that actually execute the
   **Rust-backed** process/scheduler code. A Rust build success alone is
   insufficient.
5. **Rollback:** keep the original C implementation behind `RUST_CORE=0`
   until the Rust-backed kernel has passed the same real-user-mode QEMU tests.
   Do not remove C/assembly reference implementations early.
6. **Safety:** never test on physical storage. Rust does not automatically
   make unreviewed paging, MMIO, IDT, user-copy, DMA or `unsafe` kernel code
   memory safe.

## R001 — Process scheduling and user fault policy

Implemented sources:
- `rust/kernel_core/Cargo.toml`, `src/lib.rs`, lockfile
- C ABI replacements for `process.c`, `scheduler.c`, and
  `user_fault_policy.c`: PIDs, PCB state transitions, CR3 ownership,
  round-robin selection, yielding, tick accounting, kernel/user fault check
- Rust layout tests (including `repr(C)` size/offset assertions), native
  original C ABI tests against Rust, and a bare-metal `staticlib` kernel link
- `make test-rust-host-abi`, `make kernel RUST_CORE=1`,
  `make test-rust-processes`, `make test-rust-scheduler`.
- A GitHub Actions status `indianos/rust-core-qemu` records **three
  independent results**: host ABI, kernel ELF, QEMU process/scheduler parity.

**R001 verified:** GitHub Actions [run 39543410497](https://github.com/pramod07info/indianos/actions/runs/39543410497)
reported `indianos/rust-core-qemu = success`: four initial Rust unit tests,
the original C process/scheduler/fault ABI tests, an actual ELF linked from
Rust staticlib, and both native-user-mode QEMU process/scheduler tests passed.
Default `make kernel` now uses `RUST_CORE=1`; the legacy C fallback is
available explicitly with `make kernel RUST_CORE=0`. CI requires the Rust ABI
and QEMU checks to succeed. This does not imply all kernel modules use Rust.

## R002 — Rust syscall, ELF64, and initramfs metadata

The second slice now ports three more C units, compiled by default into
the same `no_std` kernel staticlib:

- `kernel/src/syscall_abi.c` → `rust/kernel_core/src/syscall.rs`, with
  `#[repr(C)]` 24-byte syscall request and checked metadata/user-range
  validation for ABI_QUERY, WRITE, EXIT and YIELD.
- `kernel/src/program.c` → `rust/kernel_core/src/program.rs`, with a strict
  little-endian x86-64 ET_EXEC ELF64 parser; it rejects malformed headers,
  W+X segments, overlapping pages, dynamic interpreter, TLS and invalid
  entry points. A failed plan does not mutate the output.
- `kernel/src/initramfs.c` → `rust/kernel_core/src/initramfs.rs`, with
  bounded read-only archive validation and lookup, rejection of duplicates,
  malformed paths, truncated entries and trailing bytes.

These modules maintain the original C ABI, use no allocator and are tested
with Rust unit tests and the unchanged C program/user-image fixtures.
Success must also be demonstrated under QEMU, including a real boot,
ring-3 user application, guard/supervisor fault isolation, process cleanup
and the cooperative scheduler. The results are collected under the
`indianos/rust-core-qemu` status.

**Only C translation units removed from the active default kernel link** are
the six listed above. The original files remain in the repository as
reference/fallback code and still support `RUST_CORE=0`.

## Current migration inventory (2026-10-21)

| Slice | Implementation status | Acceptance evidence or remaining condition |
| --- | --- | --- |
| R001–R002 | Rust PCB, scheduler, fault policy, syscall metadata, ELF64 and initramfs validation selected by default | Original C ABI and QEMU regressions |
| R002–R003 | Rust PMM, VMM, user-image staging, guarded copy, paging policy and clone selected by default | Rust/C ABI and QEMU tests in verified CI |
| R004 | Rust Limine handoff, framebuffer, serial, GDT/IDT/TSS, exception dispatcher, PIT driver and kmain selected by default | Normal boot, ring-3, deliberate faults and integrated QEMU passed |
| R005, R008–R009 | Rust `paging_cpu.rs` and `user_runtime.rs` now own privileged CPU paging, ring-3 syscall/fault handling, process launches, M009 scheduling and cleanup | 95-step QEMU/OVMF regression suite passed on `0fc6036` |
| R006 | Native Rust user ELF demos and Rust archive/ELF/QEMU tools selected by default | C/Python reference tests remain; general-purpose userland/desktop apps are not yet implemented |
| R007/R010 | Default kernel links Rust and only two essential x86-64 assembly units; C and Python reference sources remain in the repository | Source audit now enforced in GitHub Actions; retain rollback code until independently proven safe to delete |

Limine itself is a third-party bootloader, not IndianOS-owned code to port.
Small architecture-specific exception, `iretq` and context-switching assembly
may remain in an otherwise Rust kernel.

Small architecture-specific **assembly instructions or `global_asm!`** are
legitimate parts of a Rust kernel (e.g., `iretq`, `inb/outb`, and trap entry).
Third-party Limine components are not IndianOS-owned source and retain their
upstream license.

## Reproduction after R001 acceptance

On Linux/WSL with stable `rustup`, `cargo`, Clang, LLD, QEMU, OVMF,
xorriso and pinned Limine:

```sh
rustup target add x86_64-unknown-none
make test-rust-host-abi
make kernel BUILD=build/rust-check RUST_CORE=1
make test-rust-processes
make test-rust-scheduler
```

The default `make kernel` already links the verified Rust-backed modules
and validates their behavior through CI. The default kernel no longer links C translation units. Architecture-specific
assembly remains, as do excluded C/Python reference tests and compatibility
sources, until they can be safely retired without weakening coverage.

## R004–R006 activation — Rust-first default switch (2026-10-21)

The six requested subsystems now have Rust-owned sources and are selected
by default in `Makefile` (with `RUST_CORE=1`):

- Limine revision-6 request markers, memory map/module parsing and the
  framebuffer status painter (`limine_requests.rs`, `boot.rs`).
- GDT/IDT/TSS, IST1 double-fault stack and exception diagnostics
  (`interrupts.rs`), retaining minimal assembly trap entry stubs.
- 8259 PIC / 8254 PIT driver (`timer.rs`), linked but **not enabled for
  ordinary boot**; IRQ0 preemption is still an unverified experiment.
- Kernel boot and milestone orchestration (`kernel_main.rs`).
- Native `no_std` ET_EXEC demo, worker, scheduling and fault-test programs
  (`userland/rust`), not a general desktop application suite.
- Deterministic host archive/ELF and QEMU smoke tooling (`tools/rust`);
  Python/C reference utilities remain for comparative regression testing.

`RUST_BOOT_PROTOCOL=1` omits `kernel/src/boot.c` from the linked kernel;
the `limine-requests` feature emits the protocol requests from Rust.
Default QEMU smoke tests now use the native Rust smoke CLI. Execute:

```sh
make test-rust-default-qemu
make test-exception test-pagefault test-userfault-guard
make test-userfault-supervisor test-processes test-scheduler
make test-rust-tools
# Full C reference fallback:
make kernel BUILD=build/c-reference RUST_CORE=0
```

**Verification:** [GitHub Actions run 39558378245](https://github.com/pramod07info/indianos/actions/runs/39558378245)
completed successfully for [code commit fc25814](https://github.com/pramod07info/indianos/commit/fc25814bcd42837c30eeb633c927b3f343be0267).
Rust kernel compilation, original C ABI regression tests, native Rust
tools/userland, GDT/IDT/TSS exception checks, Rust kmain and integrated QEMU
boot, faults and cooperative scheduler tests passed. This validates the
tested QEMU/OVMF configuration, not every physical computer.
Original `user_mode.c` and `paging.c`, architecture trap assembly, C ABI
reference test harnesses and Python reference tools are still present.
Consequently, **this is not a 100% Rust-only operating system**.

## R008 — privileged paging migration (2026-10-21)

`rust/kernel_core/src/paging_cpu.rs` now owns CPUID/EFER/CR0/CR3/CR4
detection and the gated shadow-root switch. The default Rust kernel excludes
`kernel/arch/x86_64/paging.c`; the reference remains available with
`RUST_PAGING_CPU=0`. The CR3 switch is QEMU-test-only and refuses PCID.
Normal boot and ring-3 CPU paging paths passed QEMU regression on
[run 39561858663](https://github.com/pramod07info/indianos/actions/runs/39561858663).
The dedicated CR3 shadow-switch roundtrip remains opt-in and is not claimed
verified by the normal regression.

## R009 — ring-3 ownership

`rust/kernel_core/src/user_runtime.rs` replaces the active C `user_mode.c`
translation unit in the default kernel. It owns syscall/fault dispatch, user
PCB lifecycle, M009 cooperative scheduling, and page-table cleanup. The
minimal `user_entry.S` trampoline remains for IRETQ/CPU register transitions.
`RUST_USER_RUNTIME=0` retains the original C runner for comparison. Verify
with `make test-rust-ring3-runtime-qemu` and the full QEMU fault/scheduler
suite. Those paths passed [CI run 39561858663](https://github.com/pramod07info/indianos/actions/runs/39561858663).

## R010 — Rust-only active kernel source audit

`make test-rust-source-audit` now verifies the default freestanding kernel
build's source/object set contains no C translation units. The only native
objects in the link are `interrupt_entry.o`, `user_entry.o`, and the Rust
`libindianos_kernel_core.a`. The GitHub Actions workflow runs this audit
before compiling the kernel. The active Rust port (prior to this audit gate)
passed [CI 39561858663](https://github.com/pramod07info/indianos/actions/runs/39561858663).
Retaining C references, host C tests, third-party Limine and essential
assembly does not violate this narrower active-kernel source goal.

## R010 — audited ELF provenance and negative tests

The initial `make test-rust-source-audit` checks Make's selected sources and
objects. The additional R010 gate writes an ld.lld linker input map
(`$(BUILD)/kernel-link.map`) for the compiled kernel and verifies the
**actual linked input objects** using the dependency-free Rust command
`indianos-tools audit-kernel`.

Exactly one Rust static archive and two permitted native x86-64 assembly
entry objects (`interrupt_entry.o`, `user_entry.o`) may contribute
objects to the default linked kernel. Extra linked C objects or archives
fail the audit. The CLI checks ELF64 x86-64 file type, archive magic and
required inputs. Rust unit tests inject forbidden C object names and
additional archives to prove rejection.

CI executes `make test-rust-link-audit BUILD=build/rust-compile` and
`make test-rust-source-audit-negative`. The latter deliberately switches
on the original C core, C main, C user runtime and C interrupt handler,
and requires all four configurations to be rejected by the Rust audit.

This verifies the active kernel link only, not third-party Limine, the
retained reference C tests, or the compiler's underlying archive
provenance. The two low-level assembly CPU transition objects remain.
Real-hardware boot and general cross-platform application compatibility
remain outside R010's scope.

## R011 — Rust-managed x86-64 assembly CPU entries

The active kernel ships the exact privileged interrupt, GDT, IRQ0,
SYS_YIELD/EXIT and IRETQ entry routines through
`rust/kernel_core/src/cpu_entry.rs` and two `include_str!`-backed
`global_asm!` source files under `rust/kernel_core/src/arch/x86_64`.
The cooperative user return frame keeps IF clear; only the `preempt`
Cargo feature selects IF=1 (0x202). GPR save order, all 32 error-code
exception stubs and the 14 exported ABI symbols remain unchanged.

`RUST_CPU_ENTRY=1` is the default and omits the two separately compiled
`.S` entry objects entirely. The kernel link is therefore **one Rust
static library input** (whose members still include low-level assembly).
The former reference `.S` files stay in `kernel/arch/x86_64/` for
`RUST_CORE=0` or `RUST_CPU_ENTRY=0` rollback builds.
`make test-rust-entry-abi` verifies symbol preservation and the R010
linker provenance gate now rejects every standalone assembly object.
Host-only `cargo test` omits privileged assembly using `cfg`.
QEMU CI boot/exception/IRQ0/ring-3/PCB/fault tests are required before
marking R011 complete; `global_asm!` does not imply those x86 instructions
were rewritten into safe Rust.

## R012 — kernel user-space boundary hardening

Effective VMM permissions are now the intersection of all four table
levels: supervisor-only, read-only, or NX ancestors cannot be overridden
by a permissive leaf PTE. User-buffer copying checks the complete range
before reading, validates each physical page against the process UserImage
ownership ledger, and rejects W+X mappings. The user-copy-ready capability
is revoked before beginning cleanup, even on a partial release error.

The ring-3 syscall/scheduler path verifies hardware-pushed CS, SS, fixed
RFLAGS and IOPL/NT/VM/IF policy, RIP and owned stack range. SYS_WRITE,
SYS_EXIT and other syscalls must originate from user-executable, nonwritable
memory. Process-launch preflight checks **all** staged pages against planned
physical frames and expected write/execute/stack rights in the detached and
cloned tables, plus the stack guard.

`make test-rust-hardening` runs adversarial unit tests for malformed
trap frames, restrictive page-table ancestors, and foreign user frames.
CI also enforces existing no_std linker/ABI, normal boot, ring-3 execution,
fault isolation and scheduler QEMU regressions. This does not imply
SMP hardening, mature hardware interrupt preemption, real-hardware support
or production-quality process isolation.

## R013 — scoped init and scripted Rust userland shell

`R013_SHELL_TEST=1` builds a freestanding Rust `/bin/init` shell and
includes a separate `/bin/worker` in the bounded read-only initramfs.
The shell's four allowlisted commands (`help`, `ps`, `run /bin/worker`,
`exit`) are executed as a deterministic **boot script**, not an interactive
shell. The single-core R013 syscall ABI adds strictly validated commands
4 (GETPID) and 5 (enqueue only known worker id 1); both are disabled
outside the `test-shell` kernel feature and cannot accept arbitrary
userspace strings or paths. Only PID1 may enqueue one worker launch.

After PID1 exits and is reaped, the kernel consumes that request, looks up
`/bin/worker` in the validated initramfs, runs PID2 in an independently
mapped ring-3 address space, and verifies successful exit, resource
reclamation, and process state transitions. This is **sequential
launch**, not a generalized asynchronous fork/exec API.

`make test-rust-shell-host` checks the parser against injection, traversal,
unallowlisted programs and invalid syscall arguments.
`make test-rust-shell-qemu` checks real QEMU serial lifecycle markers
and that both PID1 and PID2 were reaped with PMM frame counts restored.
Legacy demo/scheduler/fault QEMU regression tests must also stay green.
Interactive input, device drivers, job control and a general app loader
are separate future milestones.

## R014 — serial-backed interactive console and PS/2 polling groundwork

R014 introduces a **real input-driven** Rust userspace shell rather than a
predefined command script. With `R014_INTERACTIVE_TEST=1`, the kernel
includes a bounded, nonblocking console input driver that polls COM1 UART
(the QEMU `-serial stdio` keyboard/terminal transport) and legacy PS/2
set-1 scan codes when available. The decoder handles simple US keyboard
keys, shift, caps lock, Enter and Backspace, and discards unsupported
control bytes; it does not enable keyboard IRQ1 or provide a graphical
terminal on the framebuffer.

Syscall 6 reads one sanitized byte into a checked, **writable and owned**
user frame; no user virtual address is directly dereferenced in the
supervisor. The ring-3 shell maintains a 64-byte command buffer, echoes
characters and backspace via the serial console, and runs `help`, `ps`,
`run /bin/worker`, and `exit` only when Enter is pressed. Unknown and
oversized input is bounded. The worker request remains queued until
the shell exits, when the existing R013 kernel launch/reap path runs PID2;
there is not yet asynchronous process launch while the shell runs.

`make test-rust-interactive-host` tests PS/2 scan decoding, invalid
characters and writable read syscall permissions.
`make test-rust-interactive-qemu` starts QEMU/OVMF, sends four
independent real serial input lines (including a backspace correction)
through the emulator's stdin, validates shell output and verifies
that both processes exited with their PMM frames restored.
The normal boot mode and R013 scripted shell continue to be available.

This is a serial terminal and polled keyboard groundwork, not yet a
framebuffer text console, IRQ-driven keyboard driver, USB HID stack,
live fork/exec or PCI/storage implementation.

## R014.2 — PS/2 IRQ1 and framebuffer shell text

In the opt-in `R014_IRQ_TEST=1` variant (implies
`R014_INTERACTIVE_TEST=1`), IndianOS installs an IDT gate for vector
0x21, remaps the legacy 8259 PIC, unmasks only IRQ1, enables the first
PS/2 controller channel and accepts user-mode hardware interrupts (IF=1).
The minimal Rust-archive assembly stub saves/restores all GPRs, while the
Rust interrupt dispatcher decodes set-1 scan codes and queues printable
bytes in a bounded 127-character ring. IRQ1 acknowledges its PIC EOI,
and the existing syscall 6 drains the queue under its interrupt gate.

A text console renders a bounded 5x7 bitmap glyph set onto the validated
32-bpp Limine framebuffer, supporting a prompt, newlines, backspace and
scrolling. Serial logging remains available, as does the R014.1 polling
mode, with no IRQ1 setup on ordinary boots. On the experimental
IRQ1 mode, the first process cannot pass its QEMU acceptance check unless
the kernel records at least one physical PS/2 keyboard interrupt.

`make test-rust-keyboard-host` checks glyphs, FIFO bounds, PS/2 decoding
and user-space syscall permissions. `make test-rust-keyboard-qemu`
injects keypresses through the QEMU HMP virtual keyboard, checks shell
command output, and requires an IRQ1 delivery event plus process reaping.
This tests a legacy QEMU PS/2 keyboard, **not** modern USB HID or hardware
keyboard support, and does not make application spawning asynchronous.

The R014.2 legacy IRQ acceptance check uses QEMU `--machine pc`,
matching the existing PIC/PIT IRQ0 regression. `q35`/I/O APIC keyboard
routing is not yet claimed as supported.
