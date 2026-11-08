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
Clean runtime completion also requires the surface retirement verifier: both
previously populated streams must be retired, their counts cleared and every
service grant revoked. A restart clears this evidence before admitting new
clients.

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

Explicit compositor presentation and keyboard policy transitions cancel any
old pointer drag anchor while retaining the physical button latch. A held
button cannot keep moving the previously captured window after focus,
minimization or launcher changes; release and a fresh press are required to
capture again. The userland window-policy regression suite is included in the
normal Desktop GitHub Actions job.

New state-only unit tests cover page geometry, payload-only clearing, restart
address stability, foreign-window denial, retirement scrubbing and submission
rollback. These tests use independent local service instances. They have not
been executed locally in this batch, following the requested code-only
workflow. Compilation, these tests, actual guard mapping checks and QEMU
startup/restart/durable-shutdown acceptance remain required GitHub CI evidence;
this document does not mark an M1 gate accepted.

## Focused input and scrubbed teardown

An open Terminal editor no longer captures keys after a physical mouse click
focuses Files. PID1 uses the compositor's acknowledged front window for every
key delivery, retaining the unfinished Terminal line while its window is
unfocused. Clicking Terminal restores delivery to that same line. Desktop
accelerators remain session-manager actions; holding an editing session does
not grant the Terminal keyboard focus. Physical focus changes and suspended
editor input are logged so QEMU acceptance can exercise this real path.

The normal surface teardown verifier now records which nonempty owned streams
were retired before scrubbing. It requires both retirement bits, empty counts,
a presented frame and revoked service grants, rather than requiring nonempty
counts after teardown has erased them. This fixes the verification API's
internal contradiction; the normal boot entry currently does not call that
legacy-compatible API, so this is not evidence of a previously failing boot.
Restart clears the retirement evidence with the other session metadata.

New tests cover a partially typed command surviving focus loss without accepting
foreign-window keys, complete versus empty teardown, repeated/foreign
retirement and restart evidence reset. No local tests or binaries were run.
Pending acceptance includes real physical mouse focus transitions with a
suspended editor and rebooted file readback, negative memory-isolation probes,
service fault/restart recovery and a clean required CI run. Full pixel-buffer
surfaces and event-driven scheduler parking remain separate architectural work;
the current service IPC and physical-input loops still use cooperative polling.

Normal PID1 startup uses cooperative receive syscall 35 to join all three
authenticated service handshakes in any arrival order. An empty queue waits
instead of causing nonblocking-receive exit 142; the kernel startup dispatch
budget remains the timeout authority. Exact sender/name/length matching and
duplicate rejection prevent partial or forged readiness. Protocol tests cover
all six arrival orders and missing, duplicate and malformed handshakes. These
tests were authored without local execution; the missing-handshake QEMU job is
the required integration check for dispatch-budget recovery.
