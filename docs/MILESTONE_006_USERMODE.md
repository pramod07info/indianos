# Milestone 006 (experimental): first x86-64 ring-3 execution infrastructure

**Status:** experimental ring-3 kernel path and a separate syscall-enabled `/bin/init` test fixture are committed to `main`. The freestanding test ELF was independently compiled and inspected locally, but **successful QEMU/OVMF ring-3 execution remains unverified**. Normal boots still use the inert fixture. Do not boot IndianOS on physical machines.

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

The **normal** `userland/demo/entry.c` remains an inert loop intentionally. The separate `userland/demo/ring3_entry.S` is selected exclusively when `USERMODE_SELFTEST=1`. It uses three software interrupts: write the greeting, exit successfully on the expected return length, otherwise exit with status 1.

The experimental target builds and validates this ELF before constructing the QEMU image. The opcode precheck is only a preflight—not proof that user code ran. The QEMU transcript must demonstrate entry, greeting and successful exit.

```sh
make test-host
make test-ring3-elf   # builds & inspects syscall-enabled ET_EXEC without QEMU
make test-usermode    # QEMU/OVMF/Limine required; not yet confirmed green
```

The GitHub workflow builds `test-ring3-elf` on pushes and also attempts `make test-usermode` in QEMU as a **non-blocking experimental CI step**. Its transcript is uploaded as `indianos-ring3-experimental-log` for inspection. `workflow_dispatch` retains an explicit `run_usermode=true` option. An artifact or successful compilation is not a substitute for verifying the transcript shows entry, greeting, and successful exit. No such successful transcript has been independently confirmed yet.

## Verification gaps

- Source updates and matching tests are present in GitHub. Runtime success has **not** been verified; no GitHub Actions green run has been established for this milestone.
- Actual ring-3 entry, interrupt transition, kernel-only memory isolation, SYS_WRITE pointer handling, exit recovery and safe cleanup require a real, fully working QEMU trace. A C host test or ELF compilation alone is not sufficient proof.
- The implementation does not handle a crashing user process: unexpected CPU exceptions halt the entire kernel. This is **not** a secure user-process scheduler.
- No per-process address-space lifecycle, timer/scheduler, APIC, SMP, ASLR, user process kill, application permissions, signal handling or standard libc.
- The kernel is experimental C/assembly and has not been audited for release safety; no consumer OS security guarantee is implied.

## Next bounded action

Run the syscall-enabled `/bin/init` QEMU ring-3 image, verify it cannot read/write protected supervisor mappings or the unmapped guard page, and independently inspect the exact CI transcript. Only after successful runtime testing should we call the first native IndianOS application **executed**.

The repository owner's workflow preference remains direct-to-`main` commits; no PR was created.
