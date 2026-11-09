# Uncertain storage sessions cannot acknowledge clean shutdown

The existing storage stack already supplies explicit `BlockDevice::sync`, ATA
FLUSH CACHE, geometry-bound block windows, COW data-before-metadata barriers and
`HierVfs::sync`. This change does not duplicate those APIs or add a successful
default barrier. Unknown block drivers continue to return an error for sync.

`DurableFs::sync` now retires its mounted instance after **any** failed sync
step, including preflight head reads, complete payload health verification and
post-barrier readback. Previously the explicit flush failure retired the mount,
but a failed pre/post health read could leave it usable. A caller could restore
its synthetic device's read success and continue writing through the same
unverified mounted instance. Once retirement occurs, read, write and sync fail
without additional device I/O. Explicit remount is required to reselect and
verify checkpoints. A degraded volume still requires explicit recovery; fsync
never repairs, formats or silently accepts one surviving metadata copy.

The native ATA PIO driver now retains uncertain-transfer state too. An error or
timeout during read completion, write completion or cache flush permanently
retires that `AtaPio` instance. Later read/write/sync entrypoints return before
issuing another port command. A new, explicitly authorized probe is the recovery
boundary; no automatic device reset or replay can duplicate a partially executed
write. Successful operations remain reusable. Invalid LBA preflight does not
retire an otherwise live device. Failure diagnostics remain available for
explicit status inspection, without resetting or retrying a command.

The selected-volume service can use its existing `HierVfs::sync` shutdown path.
A failed barrier must never produce a clean-shutdown acknowledgment or allow a
same-session retry. Device reprobe/remount and service authority renewal are
separate trusted recovery decisions, not a successful close operation.

## Evidence

Three focused host tests pass against the real production modules. The
filesystem test measures a successful sync's read boundaries, injects failure
at each boundary before and after the actual barrier, then confirms unchanged
output, no later reads/writes/flushes and successful explicit remount/readback.
The same regression against the prior sync implementation fails: a subsequent
read incorrectly returns the 700-byte file instead of rejecting the session.
Driver tests verify guard success/range handling and refusal at the actual
read/write/sync entrypoints after injected uncertain completion, without running
privileged port instructions on the host.

Command: `cargo test --locked --offline --manifest-path
rust/kernel_core/Cargo.toml --lib --features test-r031-files-gui
storage_retirement -- --test-threads=1` — 3 passed, 0 failed.

Coordinator combined compilation and QEMU ATA/desktop persistence tests remain
integration requirements. Host callback faults are not physical power-loss
qualification. This change adds no SMP port ownership, hardware media identity,
cache correctness attestation, normal desktop claim or newly accepted M1 gate.
