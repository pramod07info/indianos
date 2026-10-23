# R025.1 — Authenticated IPC syscalls (QEMU)

This adds test-only ring-3 IPC calls using the kernel's bounded mailboxes,
and preserves the ordinary boot path unchanged.

## Syscall ABI (INT 0x80, 64-bit little-endian)

- **4 GETPID**: arg0=0, arg1=0; returns the actual active PCB PID.
- **8 IPC_SEND**: arg0=user pointer, arg1=4..68 byte packet length.
  Packet begins with a 4-byte destination PID and up to 64 payload bytes.
  Sender PID is obtained from the active PCB and matching scheduler task,
  never from userspace. Returns 0 or `u64::MAX` on denial.
- **9 IPC_RECEIVE**: arg0=user writable pointer, arg1=72.
  Returns a 72-byte record: sender PID u32, message length u32, then
  64 zero-padded data bytes. Success returns `8+payload_length`;
  empty returns zero without modifying the output buffer;
  error returns `u64::MAX`.
- The receive output is verified for the full 72-byte writable range
  *before* consuming a queued message.

The single-BSP software-interrupt gate validates the IRET frame,
executable syscall origin, whole user-buffer range, and actual staged
page permissions. Mailboxes are created by the kernel scheduler and
unregistered when a process exits or faults. No process can supply
a forged sender PID in the syscall arguments.

## QEMU acceptance

Run `make test-rust-ipc-qemu` with the usual QEMU, OVMF, Rust
x86_64-unknown-none target, Clang, and xorriso prerequisites.
Rust `ipc-a` and `ipc-b` applications run as separate ring-3 PIDs,
exchange authenticated ping/reply, reject nonexistent recipients
and invalid user pointers, verify non-destructive empty reads, and
exit with three dispatches, two yields and exactly one reap per PID.

## Outstanding R025 work

Capability-mediated per-service authorization, file handle syscalls,
a general blocking IPC interface, multi-CPU locking and persistent
service management remain future milestones. This test does not prove
a production-grade service or filesystem syscall implementation.

## R025 completion — capability-enforced IPC, file and process syscalls

The opt-in `test-r025` feature extends the previous R025.1 QEMU ABI.
This remains single-core and explicitly disabled in ordinary boots.

- **8 IPC_SEND:** packet is destination PID `u32`, capability token `u64`,
  then up to 64 bytes. Kernel validates the owner PID, target, rights and
  token against a monotonic capability registry before queueing.
- **10 CAP_LOOKUP:** `arg0=resource_id, arg1=0`. Only existing grants owned by
  the active PCB are returned; user processes cannot mint permissions.
- **11 FILE_CREATE**, **12 FILE_READ**, **13 FILE_DELETE:** a 48-byte user
  descriptor containing `token:u64`, 24-byte NUL-terminated absolute path,
  user data pointer `u64`, and length `u64`. Permissions are validated
  against process-specific flat-root paths (`/p1-*` or `/p2-*`). Payloads
  are bounded to 64 bytes; the memory-backed TinyFs volume is freshly
  formatted only for this opt-in QEMU test. Reads and writes use full user
  page ownership/copy checks rather than trusting raw pointers.
- **14 CAP_REVOKE:** a process can revoke only its own capability.
- **15 PROCESS_STATUS:** validated target PID 1 or 2 returns trusted PCB
  lifecycle state; arbitrary PID and invalid arguments are rejected.

The kernel issues the initial grants only for PID1-to-PID2 and
PID2-to-PID1 messages and for each PID's own namespace. Mailboxes and
remaining grants are removed on exit or fault. The QEMU proof uses two
independent ring-3 apps and rejects foreign paths, foreign/revoked
tokens, invalid pointers, and cross-process file operations. It
requires two yields and one reap per process, and preserves the
original R025.1 IPC regression in CI.

`make test-r025-complete-qemu` exercises the full test on QEMU/OVMF.
The main kernel CI runs this test as a required step.

**Scope:** R025 is complete against its bounded two-process *QEMU*
acceptance criteria. This is not a multiuser persistent filesystem,
POSIX API, SMP-safe IPC, general service manager, or a production
security audit. Long-running services and persistent file handles
remain under R026/R027/R031.
