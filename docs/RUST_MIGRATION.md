# IndianOS Rust-first migration — staged replacement (R001 onward)

**2026-10-21: IN PROGRESS. This is not a fully rewritten Rust operating system.**

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

**Acceptance:** this step is complete only when the status is `success`
for a commit, all required kernel checks pass, and the first successful
Rust-backed QEMU serial traces show two processes reaped and six round-robin
dispatches. Until then, normal builds use C as the default.

## Remaining migration work — NOT YET CONVERTED

| Slice | Owned implementation to port | Core acceptance |
| --- | --- | --- |
| R002 | ELF parser, syscall ABI, initramfs parser and user image staging | malformed ELF and access tests + QEMU user app |
| R003 | Physical frame allocator, memory map adapter, VMM page tables, paging clone | frame ownership, protection and failure rollback |
| R004 | Kernel entry/Limine handoff, framebuffer, serial, IDT/TSS/timer | firmware boot and deliberate CPU exception QEMU |
| R005 | Ring-3 ABI dispatcher, user-copy, process recovery, CPU-context integration | supervisor/guard fault containment, live scheduler |
| R006 | Native userland tools and applications: replace owned C entry apps with `no_std` Rust, and migrate owned Python tooling/tests to native Rust CLI/test harnesses where parity is available | deterministic ISO/initramfs + equivalent UX and test logs |
| R007 | Remove unused original C implementations and toolchain dependencies only after *every* migrated path is proven; update developer docs and guard firmware boot | full Rust-owned-source audit and QEMU regression suite |

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

The default `make kernel` will only switch to Rust-backed code once those
checks pass. Follow-ups must not interpret this R001 implementation as a
completed all-Rust rewrite.
