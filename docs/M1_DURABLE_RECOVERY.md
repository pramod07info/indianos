# Distinguish unreadable media from corrupted checkpoints

`DurableFs::mount`, `mount_healthy`, and `inspect_health` now propagate
payload block-read errors as `FsError::Io`. Previously payload verification
returned a boolean, treating a transport failure as proof of checkpoint
corruption. That could silently select an older committed generation and
later publish over the newer data after a transient read error.

Verified checksum corruption still disqualifies only the referencing
checkpoint and permits the existing explicit recovery mount to select the
surviving version. A transport error leaves checkpoint validity unknown;
the caller must resolve the I/O problem and retry, rather than automatically
rolling back or repairing. The disk format, public error variants and
no-autoformat policy are unchanged.

Fault-injection regressions fail reads in both current and previous
generations, at the first and second payload sectors. Mount, strict mount,
health inspection and explicit repair return `Io` without writes. Clearing
the read fault exposes the newest committed file. A failed multi-sector
read also leaves the caller's output untouched. Existing genuine-checksum
corruption recovery tests remain in the full suite.

Validation on the task branch:

- `cargo test --locked --offline --manifest-path rust/kernel_core/Cargo.toml --lib`:
  1,802 passed, zero failures.
- Freestanding check uses `cargo check --locked --offline --manifest-path
  rust/kernel_core/Cargo.toml --no-default-features --target
  x86_64-unknown-none --features test-r031-vfs`: passed.
- The coordinator runs combined VFS/QEMU five-boot acceptance; this task does
  not independently claim a new QEMU result.

This remains a bounded QEMU ATA COW filesystem. It does not add retries,
concurrent-writer locking, physical hardware qualification or automatic
repair of failing media.
