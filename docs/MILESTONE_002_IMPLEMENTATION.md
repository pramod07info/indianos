# Milestone 002 — exception tables and physical frame allocator

**Status: source implemented; locally compiled and host-tested. QEMU/OVMF execution is not yet independently verified.** This remains an experimental kernel, not an installable OS.

## CPU exception foundations

- Install an IndianOS-owned x86-64 GDT, with ring-0 code and data selectors.
- Install an IDT with all 32 architectural CPU exception vectors and fallback gates for unexpected interrupts. External interrupts remain disabled.
- Assembly stubs normalize vectors with and without CPU-pushed error codes and pass vector, error code and fault RIP to a terminal COM1 serial reporter.
- All exception handlers currently **halt**; they do not attempt recovery or return. No TSS/IST, APIC, timer, user mode or SMP is available yet.
- A deliberate **UD2 invalid-opcode variant** can test whether the IDT receives vector 6, without affecting normal kernel binaries.

## Physical frame manager

- Limine memory-map entries are adapted to IndianOS-defined structures; the PMM does not consume third-party boot-protocol types.
- Manage only fully page-aligned regions marked **usable**. Physical page zero, bootloader reclaimable, reserved memory, kernel image and framebuffer regions remain excluded.
- A static bitmap tracks up to 8 GiB of **usable frame capacity** (2,097,152 frames) in at most 128 usable spans. This is an early prototype ceiling, not an upper physical address limit.
- `pmm_allocate_frame()` returns a physical address but does **not** write or map that frame. A virtual memory manager must precede any page-table manipulation.
- `pmm_release_frame()` checks for alignment, ownership and double-free. Conflicting memory-map entries cause fail-closed initialization.
- The self-test allocates, releases, reallocates and checks frame counts without accessing unmapped physical memory.
- Concurrency is intentionally unsupported until there is an interrupt-safe locking and scheduler design.

## Build and test

On Linux/WSL with clang, lld, binutils, make and Python 3:

```sh
make kernel
make test-host
```

For full emulator verification, additionally install QEMU, OVMF and xorriso:

```sh
make image
make test-boot
make test-exception
```

The deliberate fault-test image is built separately beneath `build/ud2`, leaving the normal kernel unchanged.

### Normal serial milestones

```text
IndianOS: kernel entered
IndianOS: boot information validated
IndianOS: milestone 001 complete
IndianOS: exception handlers ready
IndianOS: physical memory manager ready
IndianOS: milestone 002 complete
```

Additional framebuffer messages may appear between M001 markers.

### Intentional fault path

```text
IndianOS: triggering intentional UD2
IndianOS: FATAL CPU exception vector=0x0000000000000006 ...
```

The fault variant **must not** report M002 completion.

## Verification performed

- Freestanding `x86_64-unknown-none-elf` ELF compilation/linking: **passed**.
- Separate normal and deliberate-UD2 kernel variants: **compiled**.
- Native C allocator host harness: **passed**.
- Seven Python serial smoke-parser tests: **passed**.
- Real QEMU/OVMF boot and deliberate exception path: **not run locally** because QEMU/OVMF/xorriso are absent here. CI is configured to run them on GitHub.

## Next M003 requirements

Build a safe higher-half address translation interface (HHDM only when the loader provides one), page-table primitives, isolated ownership of frame mappings, and explicit W^X/NX protections. Introduce TSS/IST before more complex interrupt handling. Do not attempt userland, networking or third-party application execution before process isolation and a syscall ABI exist.
