# M1 cooperative IPC receive continuations

The live ring-3 runtime now implements additive syscall **35**, enabled with
`test-ipc`. Existing syscall 9 remains a nonblocking polling receive.

## ABI and production behavior

- `arg0`: an entirely writable 72-byte user buffer, validated before waiting.
- `arg1`: a cooperative scheduler-round budget from 1 through 32.
- Success: `8 + payload length`, with the same wire layout as syscall 9:
  sender PID `u32`, payload length `u32`, then 64 zero-padded payload bytes.
- Budget exhaustion: zero, leaving the caller's buffer unchanged.
- Invalid identity/range/budget, stale mailbox or copy failure: `u64::MAX`.

Queued messages are copied immediately. Otherwise the kernel captures the
current authenticated Task's continuation, binds it to the mailbox registration
epoch, and skips that Task during scheduling. The first selection pass permits
at least one peer opportunity, including a one-round budget. Each later
selection pass consumes one round. Queued messages win over an expiring budget.
If all tasks wait, at most 33 bounded empty selection passes expire the waits;
these passes do not fabricate hardware ticks, scheduler ticks, yields or user
dispatches. These budgets are **not wall-clock timeouts** or hardware timer ticks.

Both polling receive and wait delivery copy before removing the FIFO head.
A failed copy retains the message in its original queue position. Writable
memory is revalidated when a saved continuation completes. Wait state is private
to the exact Task, cleared before fallible resource teardown, and cannot inherit
messages from an unregistered/recreated mailbox with the same PID.

Saved syscall continuations validate user IF against the runtime profile, just
as dispatch and resumed frames do; a cooperative call in a preempt-enabled or
IRQ-keyboard build therefore retains IF=1 rather than being mistaken for an
interrupt-disabled frame. Privilege
selectors and flag checks remain enforced.

The Process and Scheduler C layouts and existing polling wire ABI are unchanged.
The default scheduler bound remains 32 dispatches and preemption remains 2048.
The existing persistent graphical Files profile uses a finite **256-dispatch**
bound to support real per-key Terminal/broker exchanges; reaching that bound
still fails closed. A sleeping selection does not consume that bound.

## Validation and limits

Regressions cover wrapped FIFO copy failure, wire padding, every budget 1..32,
unchanged timeout output, delivery at the deadline, recycled mailbox epochs,
revoked-sender traffic, checked epoch exhaustion, user-buffer boundaries, exact
scheduler budget exhaustion and a production runtime call with every task
waiting. The all-waiting test confirms progress without synthetic timer or
dispatch accounting. Host suites and freestanding compilation are run with the
actual persistent graphical/supervisor runtime features; legacy and preemptive
scheduler bounds are checked separately.

The coordinator's real QEMU Terminal/Files acceptance must exercise this syscall
through actual ring-3 broker/client calls. Host tests alone do not establish that boot
acceptance. This remains the existing single-BSP, serialized interrupt-gate
runtime; it does not provide SMP-safe blocking, arbitrary-thread waits,
wall-clock deadlines, a POSIX signal model or normal desktop startup.
