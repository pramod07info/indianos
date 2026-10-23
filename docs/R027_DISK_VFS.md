# R027.1 — Disk-backed TinyFs integration

Only the opt-in `R015_ATA_TEST=1` QEMU build formats a filesystem.
The existing `tools/test_ata_qemu.py` runner creates a disposable,
32-sector raw IDE disk, passes it to QEMU and verifies **every byte**
has been restored before deleting the image.

The kernel snapshots sectors 16 through 20, creates a bounded
`BlockWindow` partition, mounts TinyFs, creates and reads files through
canonical absolute VFS paths, rejects traversal, remounts, reads persistent
data, deletes and reuses a file slot, then restores the original sectors.
A guard check verifies sector 15 was not modified.

Reproduce: `make test-rust-ata-qemu` with QEMU, OVMF, clang,
LLD, xorriso and Rust `x86_64-unknown-none` installed.

This is not an installer, crash-safe filesystem, or a user-mode file
service. R024 hardware timer-driven preemption remains unverified:
the prior QEMU run observed PIC IRR bit 0 pending with no in-service
IRQ and failed `indianos/preempt-qemu`.
