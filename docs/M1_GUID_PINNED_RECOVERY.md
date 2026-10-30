# GUID-pinned explicit Terminal recovery

Ordinary GPT Terminal sessions require two healthy COW metadata checkpoints.
Previously, recovering a degraded selected volume required the geometry-only
mount API, losing the disk/partition GUID binding. The persistent Terminal
service now exposes an explicit `GptTerminalSession::recover` operation.

The trusted caller supplies its original `GptSelection`, scheduler-derived
identity, current capability table, monotonic time and new session deadline.
The operation requires both `FILE_READ` and `FILE_WRITE`, a live exact process
generation and a future deadline **before any device query or sector read**.
It validates the complete GPT and original disk GUID, partition GUID and
geometry, mounts only that bounded volume, and invokes the existing explicit
single-checkpoint redundancy repair. Healthy media causes no writes. A nil
DiskGUID is rejected as invalid GPT metadata because it cannot identify media.

Success requires a fresh complete GUID and healthy-volume validation after
repair. It returns `(new_session, repaired)`, with a zero command count and
the requested deadline. It never takes or mutates an old session: a session
retired by a previous media/health failure remains retired after recovery.
The new session executes the existing authenticated durable commands.

Recovery never formats a disk or rewrites file payloads. One good checkpoint
and its payloads must verify. A write or final verification failure returns an
error and no session; a completed write followed by an error remains uncertain
until a subsequent explicit recovery/read verifies the device. The block-device
owner must serialize/quiesce media access throughout the operation, as for
existing selected mounts. Block-device writes must provide their documented
durable-sector semantics.

Regression coverage includes every byte cut (0 through 512) of the repair
sector, fresh-session readback, retired-session nonrevival, healthy no-op,
changed GUID/geometry rejection, denied identity/rights/deadline without I/O,
unformatted or unreadable media, final identity-read failure, and neighbor-sector
guards. The former synthetic GPT fixture used a nil DiskGUID; it now has a
nonzero GUID, while a separate fully checksummed test proves nil rejection.

Validation commands:

```sh
cargo test --locked --offline --manifest-path rust/kernel_core/Cargo.toml --features test-r031-vfs m1_
cargo test --locked --offline --manifest-path rust/kernel_core/Cargo.toml --features test-r031-vfs --lib
cargo check --locked --offline --manifest-path rust/kernel_core/Cargo.toml --no-default-features --target x86_64-unknown-none --features test-r031-vfs
```

This is the existing feature-gated kernel-service API. It does not wire a
ring-3 syscall, expose recovery to arbitrary applications, prove real QEMU
power-loss behavior, or complete the normal desktop/recovery milestone.
