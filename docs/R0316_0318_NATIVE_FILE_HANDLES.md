# IndianOS R031.6–R031.8 — Native filesystem handles and access rights

A single kernel/userland change set on top of R031.5:

- R031.6: four bounded, transient per-process file handles with monotonic
  non-recycled tokens, current offsets, open/read/write/seek/close, safe
  release on exit/fault and blocked use after close.
- R031.7: handle access rights (read/write), verified process ownership and
  PID2 read-only allowlist for \`/shared/\` (all other paths denied to PID2).
  PID1 remains the only writer. Handles never substitute for kernel-verified
  process identity and invalid user addresses are rejected before I/O.
- R031.8: real ring-3 native application exercises open, read, seek, write,
  close, stale-token rejection and read-only denial through syscall 33.
  A ring-3 worker explicitly tests denial for \`/etc/big\` in the final
  QEMU boot. This is *native API integration*, not yet GUI Files-app wiring.

## Syscall 33 ABI

\`arg0\` action: 1 OPEN, 2 READ, 3 WRITE, 4 SEEK, 5 CLOSE.
\`arg1\` points to a copied and strictly checked 64-byte descriptor:

| Bytes | Value |
| --- | --- |
| 0–23 | OPEN-only NUL-terminated absolute path, rest zero |
| 24–31 | opaque handle token (zero for OPEN) |
| 32–39 | OPEN mode 1=read, 2=write, 3=both; SEEK offset |
| 40–47 | READ/WRITE user buffer pointer |
| 48–55 | READ/WRITE buffer length |
| 56–63 | reserved, must be zero |

For READ/WRITE all other fields must be zero; READ copy-out is limited to
72 bytes, WRITE to 256 bytes; all operations return u64::MAX on error.
The file size remains 2048 bytes and the filesystem stays QEMU-only.

## Security model and limitations

Ownership/rights are **runtime capability protections, not persistent POSIX
permissions**. PID1 is service root; PID2 can open already-existing shared
files read-only. PID3+ are denied. Existing syscall 32 remains PID1-only.
Tokens and cursors are kernel-owned and discarded when a process terminates;
tokens are not inherited across process restart or boot. Four open handles
maximum; no SMP, actual users/accounts, on-disk ACL, secure randomness,
POSIX unlink-while-open, concurrent writers, namespaces or hardware installer.

Regression tests: \`cargo test --offline --manifest-path rust/kernel_core/Cargo.toml --features test-r031-vfs\`.
QEMU: \`make test-r031-vfs-qemu\` (five isolated boots with CRC recovery;
the fifth runs the native file manager and worker permission checks).
