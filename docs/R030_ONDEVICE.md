# R030.1–R030.2: persistent user files and native app lifecycle (QEMU)

**Integrated single-commit verification milestone.** Extends the proven R027
checksummed, dual-metadata copy-on-write filesystem into a syscall-facing,
single-core ring-3 service under explicit `test-r030-ondevice` feature.

## R030.1
- QEMU-only, exact 128-sector disposable ATA disk.
- Bounded sector window: sectors 24–123 inclusive; never probe arbitrary
  physical storage under a normal boot.
- Explicit full-blank format once, followed by mount; corrupt existing
  media fail closed, never implicitly reformatted.
- PID1-only syscall 30 for create/read/overwrite/delete on short root paths,
  48-byte copied descriptors and up to 256-byte transferred payloads.
- Kernel-owned copyin/copyout checks before disk or user memory changes.
- Files and app state survive four real QEMU reboots, with unchanged guard
  sectors outside the bounded region.
- The underlying R027 implementation supports 8 root files of up to 2 KiB,
  but this syscall endpoint limits transfers to 256 bytes. No hierarchical
  directory enumeration, multiuser permissions or filesystem mounts yet.

## R030.2
- The existing R030 SDK verifies **Ed25519-signed ELF package artifacts on the
  build host**. The QEMU runner signs and verifies the exact worker ELF before
  executing a boot image with that staged worker.
- PID1-only syscall 31 implements allowlisted worker install, version
  upgrade, rollback, remove and status via persistent records in R027.
- Kernel syscall 5 refuses to launch /bin/worker unless on-disk install
  state validates. PID2 also cannot invoke privileged installation syscalls.
- A boot-staged ELF is executed as a separate ring-3 process and is reaped
  by the verified R026 scheduler. Updates to persistent metadata do not
  dynamically replace that compiled ELF image.

**Trust boundary:** This is **not** a general on-device signed package
installer. The QEMU kernel does not yet verify Ed25519 signatures itself
or load previously unseen ELF bytes from the volume. It only grants
installation of an already allowlisted executable embedded in initramfs,
whose signature is checked by the test harness on the host. Actual
on-device signed package ingestion, streaming multi-sector ELF storage,
dynamic app loading and authenticated update rollback remain pending.

## Reproduction
```sh
make test-r030-ondevice-qemu
cargo test --offline --manifest-path rust/kernel_core/Cargo.toml --features test-r030-ondevice
```

No writes to real host disks or general hardware are performed by the
test harness. Retained QEMU images must never be marketed as production OS
installers.
