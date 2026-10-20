# M005 continuation — initramfs and detached user-image staging

**Status:** implementation merged directly to main, host-tested and freestanding ELF linked. **QEMU/OVMF boot, live CR3 activation and ring-3 user execution have not yet been verified/implemented.** Do not install IndianOS on physical disks.

## What now exists

1. A read-only, uncompressed **IndianOS initramfs v1** archive format with a bounded, deterministic Python packer.
2. A kernel-side archive reader that rejects malformed header sizes, duplicate entries, unsafe relative/traversal names, unknown flags, truncated payloads and trailing garbage. The reader never executes a file or writes to the archive.
3. A freestanding x86-64 user ELF program packaged at **/bin/init** in the emulator ISO. Limine is configured to load the archive as a boot module.
4. The kernel requests that module via its pinned Limine protocol and checks **/bin/init** before staging.
5. A detached user-image stage uses the existing ELF validation, physical frame allocator, Limine high-half direct map and four-level VMM. It allocates and **zeroes physical pages**, copies only initialized ELF file bytes, leaves BSS zeroed, and enforces separate executable/text and writable/stack permissions in the **detached** page-table tree.
6. The user stack receives 16 pages (64 KiB), and the 4 KiB guard page below it is deliberately left unmapped. A data-page ownership list tracks at most 128 user pages. The stage frees both the user data frames and the page-table frames after validation, and checks that the PMM free-frame count is unchanged.
7. A host test injects failures into allocations, checks rollback and verifies ELF bytes, BSS, user/kernel permissions, guard-page absence, and successful cleanup.

**The initial /bin/init is a no-op fixture. It does not run.** The kernel validates and transiently stages it, then destroys the detached tree. The active CR3 remains bootloader-owned. This stage **does not** establish a security-isolated process, ring 3, native app runtime, Linux ABI or Windows/macOS compatibility.

## Archive format

Uncompressed, little endian, maximum 32 MiB, 1–16 files:

- Bytes 0–7: ASCII magic INDRAM01.
- Bytes 8–11: u32 file count.
- Bytes 12–15: u32 total archive length, including header.
- For each file: u16 pathname byte length, u16 flags (must be zero), u32 file content length, pathname bytes (absolute strict ASCII), then file content bytes. No padding.
- Duplicate names, empty components, dot components, traversal, invalid characters, count overrun and any trailing bytes are rejected.

Archive format version 1 has **no cryptographic signature, authenticity check, permissions, compression, links or update format**. It is an early boot format, not a trusted package manager or production filesystem.

## New modules

- [kernel/src/initramfs.c](../kernel/src/initramfs.c) and [initramfs.h](../kernel/include/indianos/initramfs.h)
- [kernel/src/user_image.c](../kernel/src/user_image.c) and [user_image.h](../kernel/include/indianos/user_image.h)
- [kernel/src/user_image_boot.c](../kernel/src/user_image_boot.c), for an early-boot test only
- [tools/build_initramfs.py](../tools/build_initramfs.py), [tests/test_user_image.c](../tests/test_user_image.c), [tests/inspect_archive.c](../tests/inspect_archive.c)

## Build and test on Linux/WSL

Requires Clang/LLD, binutils, make, GCC or Clang for host tests, and Python 3:

~~~sh
make kernel           # freestanding x86-64 ELF (offline)
make test-host        # includes user-image and archive parser regression tests
make test-user-elf    # build standalone /bin/init fixture and validate its ELF
make test-initramfs   # pack the ELF and validate archive + ELF from host
~~~

If QEMU/OVMF/xorriso and the pinned Limine loader are installed:

~~~sh
make image            # includes boot/initramfs.img as Limine module
make test-boot        # kernel should log validated and staged /bin/init
~~~

Expected serial messages after the earlier M005 foundation markers:

~~~text
IndianOS: read-only initramfs /bin/init validated
IndianOS: initramfs /bin/init staged and released safely
~~~

Missing, malformed or unstageable /bin/init is a fatal boot failure, not an invisible success. The QEMU serial parser now requires both messages for the normal boot path.

## Validation evidence

In the local development workspace:

- Full freestanding x86-64 kernel ELF compiled and linked with Clang/LLD against a **temporary Limine compile shim** (the GitHub repo retains the complete pinned Limine header); this does not constitute a full pinned-header build.
- The production initramfs reader and the user-image staging algorithm passed Clang sanitizer (ASan/UBSan) and GCC host tests.
- A real freestanding x86-64 demo ELF was compiled and packed into a **4,601-byte** read-only archive and successfully validated by a host-side C inspector.
- The earlier 14 Python serial parser tests and 2 Python archive packer tests passed locally. The newly extended QEMU marker tests were committed but were not run against a real emulator here.
- The complete QEMU/OVMF boot, Limine module handoff and PMM-backed staging in the running kernel are **not yet confirmed**.

## Next engineering work: first actual ring-3 program

1. Verify the complete emulator boot and fix any Limine config/ABI problems in GitHub CI.
2. Build a process-owned address space with required supervisor-only kernel mappings; **do not load the detached user-only root into CR3**.
3. Establish ring-3 GDT selectors, TSS.RSP0, per-thread privileged stacks and an audited IRETQ transition.
4. Introduce a real interrupt/syscall trap (or SYSCALL/SYSRET) with safe user-pointer validation and copy-from-user.
5. Demonstrate a sandboxed process writing a serial message and exiting, with positive and malicious input tests.
6. Only then claim IndianOS can execute its first native app.

Do not run this kernel on real computers or imply it can execute arbitrary Windows, Linux or macOS apps.
