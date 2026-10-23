# R027 — Durable filesystem on a disposable QEMU ATA disk

## Delivered and actually verified

The `durablefs.rs` module implements a distinct versioned, fixed-extent
filesystem format with a bounded allocator-free `no_std` Rust core.
It operates on the existing `BlockDevice` interface, and the opt-in kernel
test uses legacy ATA PIO primary master with `BlockWindow` as a strict
sector-range limiter. **Ordinary boots never auto-mount or format a disk.**

| Property | QEMU-verified design |
|---|---|
| Files | 8 fixed directory slots, flat absolute VFS paths |
| File capacity | 2,048 bytes maximum across 4 sectors per data bank |
| Crash metadata | Two versioned, independently CRC32-checked metadata sectors |
| Copy-on-write | Three reserved banks per file; never rewrite data referenced by either valid metadata generation |
| Data integrity | CRC32 over each file's logical bytes, checked on mount and read |
| Recovery | Mount selects the newest metadata candidate whose checksums and referenced files are intact; otherwise falls back to its valid predecessor |
| Write ordering | ATA PIO uses FLUSH CACHE after every sector, data sectors before new metadata commit |
| Safety | Blank-volume-only explicit format; rejects corrupt nonblank disks and invalid file paths |
| Device test | QEMU primary-master IDE disk IDENTIFY, exactly 128 sectors and a 100-sector isolated volume |

### Reproduce

```sh
cargo test --offline --manifest-path rust/kernel_core/Cargo.toml
make test-rust-ata-qemu
make test-r027-durable-qemu
```

The R027 runner creates one temporary 128-sector raw image, uses the same
file over **five** QEMU/OVMF boots, and deletes the image after verification.
It proves the following sequence:

1. Empty temporary device is identified, explicitly formatted and populated
   with a 1,701-byte file plus an independent note and phase marker.
2. Second boot mounts the existing filesystem, checks both files, then
   overwrites the data with a 1,923-byte file and commits its new generation.
3. Third boot remounts and verifies the latest contents.
4. The host deliberately corrupts the CRC of the newest metadata sector;
   fourth boot falls back to the prior valid generation without formatting
   and re-commits its phase marker.
5. Fifth boot verifies recovery persistence and correct data.
   Every boot checks that sectors adjacent to the selected volume match
   their pre-boot values; the host checks the entire outside-disk region
   remains zero after every boot.

Host regression tests additionally inject interrupted data writes and
torn metadata commits, validate fallbacks and reject relative/traversal
paths. Both the previous TinyFs QEMU smoke and the new R027 reboot smoke
are required in the verification workflow.

[Passing initial five-boot QEMU verification](https://github.com/pramod07info/indianos/actions/runs/39929748026).

## Limits and future requirements

The durability proof assumes independent 512-byte sector atomicity and
honest successful ATA FLUSH CACHE completion in QEMU. It is **not** a
physical power-failure certification. This is neither a partitioned
disk installer nor a POSIX filesystem. It lacks dynamic extent allocation,
journaling on arbitrary devices, nested directories, 64-bit large files,
SMP locks, hot-plug, PCI device enumeration, virtio-blk, AHCI, NVMe,
permissions, encryption, or a production user-mode mount service.
R025's earlier RAM-backed TinyFs syscall service is not silently
redirected to a potentially destructive host disk.
