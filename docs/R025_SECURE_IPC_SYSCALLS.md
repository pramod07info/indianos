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
