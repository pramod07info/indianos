# Milestone 005 — user-program foundation (first implementation slice)

**Status: first slice implemented and locally host-tested. Not yet a runnable user process.** The kernel remains an experimental QEMU-only research project; do not install on real disks.

## Implemented

### Static ELF64 x86-64 load planning

`kernel/src/program.c` provides `indianos_program_plan_elf64` as a **pure parser** of externally supplied bytes. It does not load or run ELF programs. It accepts only little-endian x86-64 `ET_EXEC` binaries with a bounded ELF header and 32 or fewer program headers. It rejects dynamic/interpreter/TLS dependencies, executable GNU stack, unrecognized load flags, W+X (writable + executable) segments, bad alignment, noncanonical or disallowed user virtual addresses, overlapping pages, and out-of-file ranges.

Additional constraints:

- Maximum source image size: **32 MiB**.
- Maximum supported `PT_LOAD` segments: **8**.
- Program entry must point to initialized executable file bytes, not merely BSS.
- Segment bounds are rounded to **4 KiB** pages for planning. Overlap is rejected at page granularity, even when raw segment byte ranges do not overlap.
- User program addresses start at **0x400000** and must stay below the reserved high user-stack area.
- User-stack plan: **64 KiB**, with an additional **unmapped 4 KiB guard page** immediately below it. These are *planned*, not installed page mappings.
- Output descriptors describe segment placement and page permissions, not loaded page ownership or executable process state.
- Malformed input fails before the output plan is committed.

### Draft syscall ABI contract

`kernel/include/indianos/syscall_abi.h` reserves syscall numbers:

| Number | Operation | Metadata contract |
| --- | --- | --- |
| 0 | ABI_QUERY | Both arguments must be zero |
| 1 | WRITE | User buffer and length must lie within a planned readable region; length 1–256 bytes |
| 2 | EXIT | Exit-code argument accepted; second argument must be zero |

`indianos_syscall_validate` validates metadata only. It **does not** install a syscall CPU entry instruction, dereference user buffers, print messages, switch privileges, schedule tasks or exit running processes. The ABI is provisional and may change before the first public userland release.

An actual secure syscall implementation will need: ring-3 entry/return, per-process credentials, process-specific page tables, fault-safe `copy_from_user`, page permission enforcement, interrupt/IST/RSP0 handling, task lifecycle and per-process resource ownership.

### Boot-time check

The kernel uses a small synthetic ELF64 byte array and a synthetic syscall request to validate parser and ABI assumptions at boot. Nothing is executed. Success adds these ordered serial messages after the M004 completion marker:

```text
IndianOS: ELF64 user-image validation ready
IndianOS: syscall ABI metadata validation ready
IndianOS: milestone 005 foundation complete
```

Failure emits `IndianOS: FATAL user image contract self-test failed` and halts.

## Tests

On a development Linux/WSL host with Clang, Make, LLD and binutils:

```sh
make kernel
make test-host
```

For sanitizer coverage:

```sh
clang -std=c11 -O1 -g -fsanitize=address,undefined -fno-omit-frame-pointer \
  -Wall -Wextra -Werror -I kernel/include \
  tests/test_program.c kernel/src/program.c kernel/src/syscall_abi.c \
  -o build/m005_program_sanitized
./build/m005_program_sanitized
```

Emulator-only verification, once QEMU, OVMF, xorriso and Limine are available:

```sh
make image
make test-boot
make test-pagefault
```

GitHub Actions now includes the M005 host program test and sanitizer invocation.

### Evidence and limitations

- The ELF parser and syscall policy source passed native Clang host tests, including negative malformed-image cases.
- The same sources compiled as freestanding x86-64 ELF target object files.
- The parser and test suite passed AddressSanitizer and UndefinedBehaviorSanitizer locally.
- Firmware-to-kernel QEMU boot and the full kernel link against the checked-in Limine header are **not independently verified in this session**.
- Native, Windows, Linux, macOS and Android app compatibility remains aspirational; this parser alone does not provide any of those ABI runtimes.

## Next bounded implementation (M005 continuation)

1. Introduce a **read-only initramfs** format and an immutable built-in test application image with reproducible packaging.
2. Allocate real zero-initialized program segment pages via PMM and map them into a separate user VMM tree; leave guard pages unmapped.
3. Verify no kernel mappings leak into low user-address space; check that data/text permissions follow the ELF plan and W^X/NX requirements.
4. Design validated TSS RSP0 per-thread kernel stacks and a minimal ring-3 transition under QEMU feature flags.
5. Add an actual syscall entry path with `copy_from_user`, return-value conventions, and a terminal `SYS_EXIT`.
6. Only declare first-user-program execution complete when the **QEMU serial log** shows a ring-3 program running and exiting without bypassing the privilege boundary.

No user confirmation or PR is needed for the repository's established direct-to-`main` development workflow.
