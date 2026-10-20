# Milestone 001 — boot a minimal IndianOS kernel in QEMU

**Status:** planned, not implemented. This is the first coding milestone after Phase 0.

## Scope (keep bounded)

- Pin compatible Limine loader and protocol header revisions.
- Create a freestanding x86-64 ELF kernel with a documented linker script.
- Add boot request start/end markers and correctly initialized version requests according to the **pinned** Limine protocol.
- Define kernel entry, COM1 polling logger and fatal panic/halt.
- Read and validate required memory-map response.
- Optionally select a supported RGB framebuffer and draw a test rectangle without unaligned/overflowing writes.
- Build bootable image and run in QEMU/OVMF.
- Add one emulator smoke test that detects the serial success banner with a timeout; print useful failure diagnostics.

## Expected output

```text
IndianOS: kernel entered
IndianOS: boot information validated
IndianOS: milestone 001 complete
```

Do not print success before all the checked operations have completed.

## Proposed commands (not yet available)

```sh
make toolchain-check
make image
make run
make test-boot
```

## Acceptance checklist

- [ ] Version-pinned dependencies with source/license details
- [ ] `make image` succeeds on a documented Linux/WSL environment
- [ ] QEMU with OVMF boots without a host-disk dependency
- [ ] Serial output contains the exact three lines above
- [ ] Missing required boot info results in safe halt
- [ ] Framebuffer absence handled without null dereference
- [ ] Kernel tests do not write to physical storage
- [ ] CI builds and runs a timed QEMU smoke test
- [ ] README updated with real, tested commands

## Out of scope

Physical installation, USB boot, desktop UI, file systems, threading, multiprocessing, custom bootloader, network, keyboard and Windows compatibility.

## Follow-on M002

Exception handling (GDT/IDT), page-frame allocator from a validated memory map, and controlled page-table construction.
