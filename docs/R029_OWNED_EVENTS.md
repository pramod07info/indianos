# R029.1: process-owned GUI operations and bounded focus events

**Implemented scope:** a no-allocation kernel policy broker binds existing
window IDs to kernel-authenticated process IDs, checks drawing/focus/status
against these grants, and queues bounded focus events. An authenticated
ring-3 client can poll `SYS_GUI_EVENT_POP=21` with a writable 16-byte
buffer. The return is 16 when delivered, 0 when empty and `u64::MAX` on
bad memory, length or identity. The record is four little-endian u32s:
`sequence, window_id, event_type=1, reserved=0`. Pointer validation and
copy occur **before** acknowledging an event. Owner teardown removes grants
and its pending events; capacity and sequence exhaustion fail closed.

**Regression:** host unit tests exercise cross-PID denial, spoofed
window IDs, FIFO, refusal to consume events on invalid acknowledgements,
queue overflow and owner teardown. A QEMU ring-3 test retains the original
two-window real VGA screenshot comparisons and asserts focus-event
delivery, invalid pointer/length denial and empty-queue behavior.

**Limits:** the QEMU demo still has *one* trusted PID1 owning both windows.
The compositor and physical input remain in ring 0. This is NOT a complete
user-space window manager, multi-app isolation or a hardware desktop.
Future milestones must wire ownership teardown into real process exit,
give distinct GUI applications distinct PIDs, route physical keyboard/mouse
events to those processes and move the compositor to an isolated process.

Run: `cargo test --offline --manifest-path rust/kernel_core/Cargo.toml
--features test-r029-userspace`, `make test-rust-host-abi`, and
`make test-r029-user-surfaces-qemu` with QEMU/OVMF installed.
