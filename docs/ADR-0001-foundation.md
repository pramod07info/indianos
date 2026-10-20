# ADR 0001: IndianOS foundation

- **Status:** proposed for Phase 0
- **Date:** 2026-10-20
- **Decision owners:** IndianOS maintainers

## Context

The long-term objective is an India-oriented PC operating system with a familiar but original desktop. Reusing Linux would greatly shorten time to market, but the project has deliberately chosen to research and implement an independent kernel.

## Decision

1. Start with **x86-64** under **QEMU**, with UEFI (OVMF) as the primary firmware.
2. Implement an **independent, modular-monolithic kernel**, in **freestanding C11** and minimal x86-64 assembly. Keep user services and UI out of kernel space.
3. Use the maintained **Limine bootloader and documented boot protocol** for early handoff. Pin versions/revisions when implementation is introduced; do not copy a random unversioned header.
4. Build through a cross-capable Clang/LLVM or x86_64-elf GCC toolchain, using explicit freestanding flags and kernel linker script.
5. Write **serial-first diagnostics** and QEMU acceptance tests before attempting persistent storage or physical installation.
6. Adopt a boot protocol adapter so that a future custom loader does not change most kernel modules.

## Why a modular monolith initially?

It avoids early microkernel IPC/context-switch complexity when we have neither memory isolation nor scheduler, while enabling later user-space drivers and services. This is a starting architecture, not a permanent constraint.

## Alternatives rejected for Phase 1

- Linux fork or desktop distro: does not satisfy the independent-kernel goal.
- Own UEFI bootloader first: high engineering cost and delays kernel validation.
- Rust-only kernel at outset: viable, but selected C for straightforward ABI/toolchain bring-up; memory-safety auditing and possible Rust subsystems remain future decisions.
- Real laptop first: excessive drivers and potential data-loss risk.

## Consequences

We must implement memory safety, isolation, device interfaces and hardware support ourselves. There will be **no** instant Windows software/driver compatibility. Commercial-grade maturity is a long-term goal, not an initial deliverable.

## Revisit when

Kernel entry + serial boot is reliable, or before introducing SMP, driver ABI, app ABI or physical-hardware beta.

## References

- https://github.com/limine-bootloader/limine
- https://github.com/limine-bootloader/limine-protocol
- https://github.com/limine-bootloader/limine-c-template
- https://www.qemu.org/
