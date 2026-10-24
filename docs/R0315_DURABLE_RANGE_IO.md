# IndianOS R031.5 — Durable offset-based file I/O

Adds offset-based reads and writes to the bounded R027/R031 durable hierarchical
filesystem without changing disk layout. This milestone deliberately retains
**eight fixed objects** and **2048 bytes per file**, not unlimited storage.

## ABI and security
- PID1-only syscall 32 supports actions 10 (read at offset) and 11 (write
  at offset). Its copied 64-byte descriptor retains path[0..24], pointer at
  [48..56] and buffer size [56..64]. The former second-path field has a
  little-endian u64 byte offset in [24..32], while [32..48] must be zero.
- Reads return the number of bytes copied, **0 at EOF**, or u64::MAX for
  invalid arguments. Writes return bytes committed, or u64::MAX on failure.
- Reads use at most 72 bytes per syscall due to the existing validated
  user copy-out ABI; writes permit at most 256 bytes per syscall.
- No sparse writes: offset must be <= current file length. Reject
  zero-length writes, arithmetic overflow and any end > 2048 bytes.
- The full existing payload is verified by checksum **before** read copy-out
  or modification. The complete new image is written to an unused bank and
  published through one CRC-protected metadata-generation update.
- Same ring-3 ownership, pointer checks and process isolation as R031.3.

## Verification
- Rust tests cover a 2048-byte file assembled in chunks; 512-byte sector
  boundaries; in-place overwrite, append and EOF; reboot persistence; old
  small-buffer rejection; hole/overflow rejection; and interrupted writes
  retaining the old complete file after remount.
- \`make test-r031-vfs-qemu\` performs **five QEMU/OVMF boots** on the same
  bounded scratch disk. Boot 3 corrupts latest metadata for recovery in
  boot 4. In boot 4, real ring-3 PID1 writes and patches a 2048-byte file
  through 64-byte transfers. Boot 5 verifies the *entire* contents and
  patch across a true reboot, rejects overrun, and confirms EOF behavior.
- The dedicated R031.5 GitHub Actions workflow also runs host ABI tests and
  source audits. Full main-branch kernel regression runs separately.

Still experimental: no true streaming descriptors, sparse files, POSIX
semantics, concurrency, dynamic free space, NVMe/AHCI or real PC installer.
