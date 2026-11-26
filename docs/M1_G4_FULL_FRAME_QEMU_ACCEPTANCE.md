# G4 full-frame and compositor-only recovery acceptance

This extends the existing normal-desktop QEMU matrix on PR #816. Source
baseline is `e7f06b750f4ccdd075bf9f778c9c11c082af02ed`; none of the host
oracles below establishes guest behavior without the integrated live run.

## Full-frame presentation and cost

Before normal readiness, and for subsequent presentation, the serial oracle
requires complete ordered records from the kernel's committed frame handoff,
PID4's measured work and PID4's publication. Missing, stale, repeated,
prefixed, incomplete or reordered records fail. Guest TSC cycles, native
transport-call count and full-frame pixel count must be nonzero and bounded.
The two ordinary boot cases additionally compare the pixel count with the
actual screendump dimensions and retain `desktop-N-startup-cost.json`.
The JSON separates host wall time from QEMU launch to observed READY from
guest frame cycles; the first frame is not treated as the entire boot cost.

Physical pointer helpers wait for the production broker's exact fresh
post-presentation x/y/button record before the next movement, press, release
or screenshot. They retain the original glyph/drag expectations and 20-second
deadlines. Fixed sleeps cannot stand in for completion of a rendered frame.

Existing independent raster/glyph, focus, drag, minimize/restore, partition,
cold-reboot and shutdown oracles remain active. A publication marker cannot
replace actual framebuffer comparisons. The kernel marker must originate
only from the new complete-frame commit path.

## Live PID4-only replacement

The existing transient compositor case now holds a real Files read lease and
commits `revived=saved` through Terminal before fault injection. It captures
a Files titlebar with the physical mouse button held. An external debugger
stops the production `indianos_desktop_frame_row` entry only in verified
ring-3 PID4, at row 1 of a bounded frame with a nonzero token, owner epoch and
request sequence. The first row has already been submitted. Before upload, the debugger changes
one nonblack pixel only in PID4's verified private user RW/NX row, clearing a
set native RGB bit. Peer roots must not alias that physical page. This produces
a valid pixel distinct from the visible frame without changing production
code, storage or page tables. The screendump must still equal the pre-frame
visible image, proving the changed staged pixel stayed private.
The debugger checks source-ISO executable bytes, page ownership and guards
before injecting a real page fault at an unmapped address.

Replacement must show exactly one fresh prepared/ready sequence, a rejected
retired frame token/epoch, and full-frame presentation. It must not repeat
normal session readiness or enter full-session recovery. Privileged receipts
prove the first three successful-image generations, CR3s and filesystem
incarnations remain unchanged, while PID4's image generation increases. The
app canvas ledger must remain byte-identical. Two still-held pointer motions
must not recapture or drag Files before release. The interrupted launcher
toggle must complete once. Physical input and Terminal commands must resume;
Files must verify its original held token after a commit. Durable shutdown
must exit/reap the original clients once and the two PID4 incarnations twice.
Existing kernel and app guard inspections remain required after cleanup.

## Replacement failure cases

Three additional disposable VM cases mutate only the future compositor ELF
source through the external debugger after the current PID4 has loaded its
private image. They corrupt the source ELF magic, substitute a yielding entry
that withholds readiness, or substitute UD2. They then inject the same live
row-1 page fault. These are separate test fixtures, not production fault APIs.
Each case requires the actual original fault, bounded local retry/fallback,
no fresh desktop READY, a read-only recovery console and byte-identical media
from fault injection onward. The withholding case must exhaust the explicit
compositor readiness budget; the repeated-fault case must execute the second
fault. Existing startup-fault and whole-session retry oracles are unchanged.

## Evidence and limitations

The edited harness has 73 focused host tests and the full repository Python
suite has 220 passing tests. They check the oracles and fixture boundaries,
not guest success. Live QEMU, freestanding, kernel, security, durable-VFS and
exact-head CI remain coordinator integration responsibilities. Retain the
candidate SHA, job URL, serial/debugger logs, images, cost JSON and artifact
ID before accepting this slice. Formal G4 acceptance and the accepted M1
count do not change from these tests alone.

The baseline e7f06b7 reliability run 45620230115 failed its existing shutdown
exhaustion case when the retry entered `desktop-display-unavailable` before
its second READY. Artifact 15025301831 retains that failure. This change
preserves the existing shutdown exhaustion assertion; a new integrated run
must prove the production fix and the two real armed shutdown attempts.

Local cloud integration found and resolved two acceptance-transport assumptions:
physical motion is now sent in at most 100-unit transfers with exact presented
coordinate/button acknowledgments, and the debugger validates either contiguous
or compiler-split storage for the existing compositor ELF tuple. These changes
retain the original glyph, capture, authority and media assertions.
