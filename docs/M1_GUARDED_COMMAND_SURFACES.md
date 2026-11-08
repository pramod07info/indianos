# Normal Desktop retained surfaces and lifecycle

Normal Desktop retains one private command stream for Files and one for
Terminal. Each stream occupies a page-aligned three-page allocation: a lower
guard, a 4 KiB payload, and an upper guard. The payload contains the existing
eight bounded drawing commands and zeroed padding. These are the canonical
streams used by submission and repaint, not an unused duplicate allocation.
Legacy experimental profiles retain their existing compact layout.

`gui::surface_guard_layout()` reports the two kernel-private layouts to the
runtime as `(lower_guard, payload_begin, payload_end, upper_guard)`, with
`upper_guard == payload_end`. The runtime must remove both guard mappings from
the source address space before cloning service roots, validate the resulting
mappings, and reject changed geometry on restart. Addresses are never returned
to user processes. A successful Rust layout test alone does not prove that
hardware page protection has been installed.

The store deliberately has no `Copy` or `Clone` implementation. Submission
rollback saves one payload's command entries; snapshots copy only command entries. Restart clears
entries and padding in place and resets metadata without assigning over the
existing guarded store. Client retirement scrubs only that client's payload and
revokes its broker ownership. A foreign or retired client cannot repaint that
window, and a failed submission preserves the previously retained stream.

The independent ring-3 compositor owns window policy. The privileged raster
backend still clips and draws commands into the framebuffer. This design does
not implement shared application pixel buffers, a GPU driver, or complete
compositor fault containment outside the existing single-BSP runtime contract.

PID1 calls lifecycle syscall `38(0, 0)` after the Files, Terminal and compositor
handshakes and the initial presentation, before logging desktop readiness.
Shutdown calls `38(1, 0)` before sending any service exit request. Failure keeps
the session running and reports that synchronization failed. The runtime owns
the authoritative readiness, selected-volume synchronization and clean-exit
state. Other services probe that they cannot invoke either operation; PID1
also probes premature shutdown, malformed arguments and replayed readiness.
Legacy experimental shutdown retains its previous interface.

New state-only unit tests cover page geometry, payload-only clearing, restart
address stability, foreign-window denial, retirement scrubbing and submission
rollback. These tests use independent local service instances. They have not
been executed locally in this batch, following the requested code-only
workflow. Compilation, these tests, actual guard mapping checks and QEMU
startup/restart/durable-shutdown acceptance remain required GitHub CI evidence;
this document does not mark an M1 gate accepted.
