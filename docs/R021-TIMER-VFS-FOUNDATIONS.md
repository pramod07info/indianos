# R021 — APIC session accounting and allocation-free VFS paths

This step improves two prerequisite foundations; it **does not** claim verified
timer-driven ring-3 scheduling, disk VFS integration or a graphical desktop.

## APIC timer safety

- Reject a second `x86_64_xapic_timer_start_calibrated` call while the
  periodic timer is already running; callers must stop before restarting.
- Reset APIC IRQ and ring-3 preemption evidence on each validated start,
  before unmasking the timer. A successful previous session cannot make
  a newly started session look operational.
- Refuse to count ring-3 preemption after the timer has stopped.
- Require positive observed IRQs and preemptions with preemptions <= IRQs.

The observed counters remain diagnostics, not proof of fair task switching.
True APIC preemption still requires QEMU execution with a UC MMIO mapping,
calibration, IDT vector, and two CPU-bound user programs without cooperative
yield calls.

## VFS path boundary

`vfs_path::normalize_absolute` canonicalizes repeated slashes and `.`
segments into a caller-provided output buffer. It rejects relative paths,
NUL, any `..` traversal, components exceeding 63 bytes, and buffer
overflow. Validation completes before any output is written. It is
`no_std`/no-heap, has host unit regressions, and does not perform I/O.
Filesystem integration and mount-level authorization remain future work.

## Validation

The isolated verification workflow runs kernel-core Rust unit tests, a
freestanding no_std build, Agent X policy tests and source-ownership audit.
The mandatory full kernel CI must still run on the final `main` commit.
