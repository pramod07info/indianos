# G4 complete-frame handoff

The normal desktop retains its guarded Files and Terminal canvases and their
atomic, generation-bound publication. PID4 now has a bounded whole-frame
transaction over the existing compositor syscall37. Its kernel counterpart
validates authority, snapshots and native packed RGB, and copies completed pixels
to the hardware framebuffer. It does not assemble background, chrome, application
pixels, pointer or stacking policy. Normal `redraw` and the old apply-and-paint
ABI cannot invoke the legacy scene renderer; legacy and memory-backed raster
fixtures retain that implementation.

Operations 0 and 2 retain the existing state and seven-pixel snapshot ABI.
Operation3 uses the same IPR1 request and sixteen-byte snapshot header with up to
768 covered pixel samples (6,160 bytes). A dedicated compositor copy path validates
every page's mapping, image ownership and permissions before copying. The generic
256-byte input and 72-byte output limits remain unchanged. Uncommitted app pixels
remain inaccessible, and published generations must match exactly.

Operations4–7 use a 128-byte IFR1 packet: width/height at4/6, row/count at8/10,
format1 at12, token16, epoch24, pixel pointer32, byte length40, sequence48,
reserved56–63 and desired DesktopFrame64–127. BEGIN requires only geometry,
epoch, sequence and desired state; it returns a token. ROW requires exactly the
next full-width row and zero desired-state bytes. COMMIT and ABORT require zero
row/payload fields. Checked geometry is 640–1600 by480–1600, with at most6,400
input bytes per row. A kernel-private staging plane covers the maximum geometry;
no full-frame userspace ELF allocation or framebuffer mapping is needed.

BEGIN validates desired window geometry without changing committed policy. It
pins both committed app generations, the previous window state, pointer position,
physical button latch and compositor epoch. Every row and commit rechecks those
snapshots. Rejected, incomplete or stale work leaves visible pixels and committed
window policy unchanged. COMMIT changes policy and presents only after every row
has arrived. The hardware copy is serialized in the syscall; this is logical
publication, not a claim of display-vblank synchronization or tear-free GPU scanout.

INFO operation8 returns geometry0–3, format4–7, native red/green/blue masks8–19,
physical button bit20–23, owner epoch24–31, committed sequence32–39, retired
last-issued token40–47 and retired epoch48–55, and a kernel-sampled guest TSC at56–63. Counter sampling stays privileged,
so inherited CR4.TSD cannot fault a compositor merely for recording frame cost. Only authenticated
PID4 may inspect it. Retirement cancels pending work and capture, advances epoch
without wrapping, and preserves client grants, canvases and the physical button
latch. A replacement may re-present the exact last committed policy at the same
sequence once in its new unpublished epoch. Ordinary frames require increasing
sequence numbers. Successful presentation records its epoch before readiness can
be acknowledged. Full-session reset clears this recovery receipt and sequence
while preserving monotonic token allocation and advancing epoch; exhaustion fails
closed before reset changes anything.

Host verification on this transport source: **4,228 normal-desktop kernel tests
passed**, including six new complete/incomplete publication, malformed/foreign
row, changed app/button/pointer snapshot, epoch replay/exhaustion, bulk coverage,
wire geometry and cross-page copy tests. Freestanding normal-desktop checking
passed. Existing compiler warnings remain. Exact integrated QEMU, graphical
readback, live recovery, candidate/merged-main evidence and independent review
are still required; this document does not accept G4.
