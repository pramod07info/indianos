# Milestone 006 (experimental): first x86-64 ring-3 execution infrastructure

**Status:** kernel-side source and host-test scaffolding committed directly to main. NOT a proven working user-mode process. The existing `userland/demo/entry.c` is still an infinite-loop ELF, and the syscall-enabled fixture update is blocked; QEMU runtime verification is outstanding. Normal boots are unchanged. Do not boot IndianOS on physical machines.

## Implemented (gated)

- 64-bit ring-3 code/data GDT selectors (0x2b/0x33) and TSS.RSP0 pointing to a dedicated 32 KiB privileged interrupt stack.
- IDT vector 0x80 registered as a software interrupt gate at DPL3, while all existing exception vectors retain their protected attributes.
- x86-64 assembly entry trampoline prepares an `IRETQ` frame to reach a user ELF entry point with interrupts disabled. A parallel software interrupt trampoline saves general registers, calls a C dispatcher and restores registers on `IRETQ`.
- Experimental `SYS_ABI_QUERY`, `SYS_WRITE` and `SYS_EXIT` dispatch through the existing ABI validation policy. `SYS_WRITE` uses a bounded maximum of 256 bytes and copies via verified user-page translations and the HHDM: it does not dereference arbitrary user virtual addresses.
- `SYS_EXIT` restores the original CR3 and caller's kernel stack via an assembly trampoline. Only then may the prototype reclaim the temporary page tables.
- `paging_clone_kernel_half` clones only the higher canonical 256 PML4 entries and clears the U/S bit in the new high-level entries. Lower-half entries are initially absent. A checked attach/detach routine temporarily aliases the existing user-image PML4 children without transferring page-table ownership. It detaches them **before** destroying either hierarchy.
- Ring-3 experiment is guarded by `INDIANOS_TEST_RING3` and also requires paging preflight, disabled PCID, HHDM, an initialized user interrupt stack and checked kernel instruction/data/stack translations.
- The existing read-only initramfs and PMM-backed user ELF staging are reused for the opt-in experiment. They remain detached and immediately released in ordinary boots.

## How to run available checks

On Linux/WSL with Clang/LLD/binutils/make:

```sh
make kernel
make test-host
make test-user-elf
make test-initramfs
```

With QEMU, OVMF, xorriso and the pinned Limine loader:

```sh
make test-boot
```

The experimental target is:

```sh
make test-usermode
```

**Important blocker:** This target currently fails fast during `tools/check_usermode_fixture.py`: the repository's existing `/bin/init` fixture does not contain the software-interrupt syscall instruction and loops indefinitely. A repository write restriction prevented updating that program during this implementation. The guard prevents misleading QEMU hangs and does not establish a working user application. A test-binary opcode precheck only guards an obvious omission; the eventual QEMU transcript must also show a message and a successful exit.

## Verification gaps

- Source updates and matching tests are present in GitHub. Runtime success has **not** been verified; no GitHub Actions green run has been established for this milestone.
- Actual ring-3 entry, interrupt transition, kernel-only memory isolation, SYS_WRITE pointer handling, exit recovery and safe cleanup require a real, fully working QEMU trace. A C host test or ELF compilation alone is not sufficient proof.
- The implementation does not handle a crashing user process: unexpected CPU exceptions halt the entire kernel. This is **not** a secure user-process scheduler.
- No per-process address-space lifecycle, timer/scheduler, APIC, SMP, ASLR, user process kill, application permissions, signal handling or standard libc.
- The kernel is experimental C/assembly and has not been audited for release safety; no consumer OS security guarantee is implied.

## Next bounded action

Finish a real freestanding `/bin/init` that uses the draft `int 0x80` syscall numbers to print a message and exit. Then run the QEMU ring-3 image, verify it cannot read/write protected supervisor mappings or the unmapped guard page, and independently inspect the exact CI transcript. Only after successful runtime testing should we call the first native IndianOS application **executed**.

The repository owner's workflow preference remains direct-to-`main` commits; no PR was created.
