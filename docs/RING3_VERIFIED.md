# Verified first ring-3 IndianOS application (2026-10-21)

## Result: PASSED in QEMU/OVMF

The first native IndianOS freestanding user-mode program is now **observed executing under QEMU**, not merely compiling. It enters CPU ring 3 via an `IRETQ` transition, requests `WRITE` with software interrupt `int 0x80`, prints a message via a bounded kernel-side user-buffer copy, requests `EXIT`, returns to ring 0, detaches borrowed user page-table entries, releases the cloned supervisor page tables, releases the user image, and confirms that the PMM frame count is restored.

**Verified code commit:** [`4fac1ad4d2e50ea5d837b1d9968ec33648066fd2`](https://github.com/pramod07info/indianos/commit/4fac1ad4d2e50ea5d837b1d9968ec33648066fd2)

**Passing GitHub Actions run:** [39529700739](https://github.com/pramod07info/indianos/actions/runs/39529700739) — QEMU ring-3 commit status `success`; all workflow jobs completed successfully.

The earlier [run 39529647623](https://github.com/pramod07info/indianos/actions/runs/39529647623) also showed the full user-mode and cleanup markers.

## Verified serial sequence

```text
IndianOS: entering ring-3 /bin/init (QEMU experimental)
IndianOS user: Namaste from IndianOS user mode!
IndianOS: /bin/init exited through int 0x80
IndianOS: ring-3 borrowed user tables detached
IndianOS: ring-3 supervisor clone released
IndianOS: ring-3 user image released
IndianOS: ring-3 PMM frame count restored
IndianOS: ring-3 user program experiment complete
IndianOS QEMU boot smoke test PASSED
```

The passing workflow also executed normal QEMU boot, the deliberate invalid-opcode exception case, the page-fault CR2 case, static ELF checks, paging/PMM/VMM C harnesses, and sanitizer tests.

## Root-cause analysis

Two independent failures had concealed runtime behavior:

1. **Pre-kernel test harness:** Shell `find ... | head` under `pipefail` could abort the experimental step before QEMU. Meanwhile another step chose `OVMF_CODE_4M.secboot.fd` arbitrarily, producing an EDK2 firmware exception before IndianOS started. Fixed by preferring explicitly known, non-Secure-Boot OVMF code images and using a temporary writable copy of their matching `OVMF_VARS` template.
2. **Post-exit page-table detach:** After the user program executed and returned successfully, strict equality of the borrowed PML4 entry against its source failed because the CPU set the hardware-managed **Accessed** bit in the active PML4. Fixed by ignoring **only** that bit during detach, while continuing to verify the child frame and all other protection flags. Regression tests simulate the Accessed-bit update and reject modified child-frame addresses.

CI diagnostics now preserve the final serial transcript on failure, explain each ring-3 preflight/cleanup phase, and publish `indianos/ring3-qemu` as an exact per-commit success/failure status. The workflow also includes a final gate to fail if this QEMU ring-3 regression reappears.

## Reproduce on Linux / WSL

Prerequisites: Clang, LLD, binutils, GNU Make, Python 3, QEMU x86-64, OVMF, xorriso and internet access to download the SHA256-pinned Limine binary release.

```sh
make test-host
make test-ring3-elf
make test-usermode
```

This is a QEMU-only milestone. The user-mode ELF comes from `userland/demo/ring3_entry.S` and is used only when `USERMODE_SELFTEST=1`; the normal demo fixture remains inert.

## Scope and remaining security work

This is **one test process** in a controlled, single-CPU, interrupts-disabled configuration. It is not a scheduler, generic ELF application ecosystem, multi-process kernel, audited sandbox, hardware-safe boot disk, or compatible Windows/macOS/Linux runtime.

Before consumer applications: implement per-process address-space ownership, process failure recovery rather than halting the kernel, read/write syscall APIs with robust copyin/copyout, asynchronous timer interrupts, scheduler/context switching, secure package loading and real-hardware tests. Continue running **only under QEMU** until the kernel is independently reviewed and hardened.
