# Milestone 006 (experimental): first x86-64 ring-3 execution infrastructure

**Status: QEMU-verified on 2026-10-21.** The first native `/bin/init` enters CPU ring 3, prints through `int 0x80`, exits successfully, and releases both cloned kernel and user page tables with a restored PMM frame count. See [passing CI run 39529700739](https://github.com/pramod07info/indianos/actions/runs/39529700739) and [verification report](RING3_VERIFIED.md). Normal boots still use the inert fixture. **Never boot this research kernel on physical disks.**

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
- The kernel is experimental C/assembly and has not been audited for release safety; the QEMU pass does not imply consumer-grade sandbox security.

## Next bounded action

Now that the QEMU user-process entry/exit sequence is verified, add deliberate negative access tests (supervisor memory, guard page), per-process fault handling and lifecycle management. Schedule or launch a second user process only after those safeguards have their own passing emulator tests.

The repository owner's workflow preference remains direct-to-`main` commits; no PR was created.

## Ring-3 hardening and emulator diagnostics (2026-10-21)

The latest direct-to-main updates strengthen the QEMU-only experiment and its observability:

- The process-root preflight now verifies unchanged supervisor mappings for the kernel's GDT, IDT, TSS, ring-0 syscall stack, double-fault stack, trampoline entry and exit, kernel dispatcher, current task metadata and caller stack. It rejects any high-half PML4 entry that is user-accessible.
- The loaded application's ELF entry must have a present, user, **read-only executable** page; its stack must be present, user, **writable and NX**; the guard page must remain unmapped.
- Before switching CR3, the kernel logs meaningful early failures such as unsupported PCID, missing HHDM, paging preflight refusal, missing TSS.RSP0, ELF staging failures or page-table clone failures. This prevents ambiguous timeout-only reports.
- The QEMU smoke-test runner now preserves a bounded tail of the serial transcript when a boot marker is missing or out of order.
- QEMU uses a temporary copy of matching `OVMF_VARS*.fd` whenever the template is next to `OVMF_CODE*.fd`, leaving machine firmware templates untouched.
- The GitHub Actions workflow now attempts the ring-3 demo on pushes and publishes a narrowly scoped commit status `indianos/ring3-qemu` with `success`, `failure` or `error`. Failure of this **experimental** step does not suppress the normal boot checks. The status links to the workflow execution and uploaded transcript.

### Commands (Linux/WSL)

```sh
make test-host
make test-ring3-elf
make test-usermode  # requires QEMU, OVMF, xorriso, Limine binary release
```

The `test-usermode` run is considered successful only after the QEMU serial transcript includes, in order: kernel readiness, ring-3 entry, `IndianOS user: Namaste from IndianOS user mode!`, successful syscall exit, and post-exit cleanup completion.

**Verification update (supersedes earlier pending notes):** GitHub Actions installed QEMU/OVMF, executed the complete program successfully, and published `indianos/ring3-qemu: success`. The job passed all tests on verified code commit `4fac1ad4d2e50ea5d837b1d9968ec33648066fd2`; see [the recorded serial log](RING3_VERIFIED.md). This is an emulator-only, single-process achievement, not proof of production security.
