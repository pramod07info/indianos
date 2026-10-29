# Retire pending outbound IPC on process teardown

Legacy IpcRouter unregister now removes both the exiting receiver's mailbox
and unread messages that process sent to every other registered mailbox.
Re-registering its numeric PID cannot make peers receive old queued traffic
as if it came from the replacement process. Other senders retain FIFO order.

Mailbox compaction walks the logical ring in place without allocation,
including a wrapped/full ring. Removed messages release capacity. Unknown
unregister requests change nothing. Public interfaces and payload limits are
unchanged; the caller still authenticates PID ownership and serializes access.
Generation-aware services should continue to use the existing ScopedRouter.
This does not turn the legacy numeric-PID router into a general SMP IPC API.

Regressions enumerate sender patterns and all head positions at depths one,
two and five, check maximum depth and empty queues, and prove stale outbound
traffic is removed while peers, payloads and refill capacity are preserved.

```sh
cargo test --locked --offline --manifest-path rust/kernel_core/Cargo.toml ipc::
```
