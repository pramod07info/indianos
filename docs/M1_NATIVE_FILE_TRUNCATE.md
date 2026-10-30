# Bounded native file truncation

Syscall 33 action 6 resizes an existing write-capable handle. The existing
64-byte descriptor is unchanged: token at bytes 24–31, new length at 32–39;
path, buffer pointer, buffer length and reserved bytes must be zero. Success
returns zero; failure returns the existing all-ones error value. Actions 1–5
retain their ABI. The maximum file size remains 2048 bytes.

HierVfs verifies the complete old payload before resizing, preserves its
prefix, materializes zero bytes on extension and publishes one existing COW
transaction. Shrinking then extending never reveals discarded bytes. A
same-size request verifies the CRC without publishing metadata. File length
queries also verify the payload rather than trusting cached metadata.

The handle dispatcher requires the current process incarnation, WRITE mode
and matching filesystem generation. Added and removed bytes both consume
the existing service-wide S0 mutation budget; successful no-ops consume none.
The cursor stays unchanged, and reads beyond a shortened EOF return zero.
Uncertain I/O retires the token without charging a successful write. Other
handles require reopening after a successful generation-changing mutation.
This remains single-BSP policy with PID1 writing and PID2 read-only access,
not persistent ACLs, concurrent writers or general POSIX compatibility.

Eight focused host regressions pass, covering zero-extension/remount, all
five extension COW write boundaries, corrupt-payload stat/no-op rejection,
wrong owner, read-only, stale identity/generation, limits, quotas and failed
write token retirement. Actual command:

    cargo test --offline --manifest-path rust/kernel_core/Cargo.toml --features test-r031-vfs m1_truncate -- --test-threads=1

The native ring-3 VFS demo now creates `/etc/resize`, exercises action 6,
checks prefix/cursor/zero-extension, rejects closed/read-only/oversized
requests and reopens for readback. The QEMU harness requires its new success
marker alongside all prior five-boot storage/isolation markers. Original
`/etc/big` persistence data is retained. Coordinator QEMU validation passed all
five boots, including the live action-6 marker and existing corruption recovery.
The first run exposed an unchanged syscall action whitelist rejecting action 6;
the coordinator fixed the ingress validator and added descriptor-boundary tests
before rerunning. No broad M1 gate is closed by this bounded API.
