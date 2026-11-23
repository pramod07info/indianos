# Application-rasterized retained surfaces

The normal Desktop reuses the existing isolated Files/Terminal processes,
PID1 input broker, PID4 window policy and framebuffer backend. It does not
create another compositor or copy the legacy font/raster implementation.
Apps retain their own bounded command model and use the shared font source
to rasterize canonical RGBA rows in their private guarded user buffer.
The privileged backend accepts pixels; it no longer reconstructs normal app
glyphs or fills from drawing commands. Legacy experimental profiles keep their
existing path.

Each app has a boot-pinned guarded allocation containing two pixel planes and
two coverage masks. The payload is 1188 complete pages between the original
lower and upper guards. The geometry is derived from the shared maximum
extent; runtime validation and QEMU mapping probes must use the actual export.
These allocations are never copied onto the kernel stack or relocated after
guard installation. Host fixtures use independent heap allocations.

`BEGIN` checks immutable client dimensions and a nonempty bounded dirty
rectangle, issues an app-owned transaction token and copies the current plane
into staging. Only one transaction may be pending for an app. `TILE` accepts
the exact next row segment, at most256 pixels, in canonical row-major order.
Zero is uncovered; `ffRRGGBB` is opaque. Unsupported alpha, bad geometry, wrong
tokens, skipped/duplicate rows and failed color conversion reject before
changing staging data or advancing its cursor.

`COMMIT` requires every dirty pixel and the same accepted metadata revision
observed at begin. It swaps the retained active index atomically and retires
the token. It does **not** redraw the framebuffer: existing PID1 IPC asks PID4
to publish the corresponding desktop frame. Until commit, framebuffer
composition uses the prior retained plane. `ABORT` scrubs staging without
altering the visible plane; incomplete or invalid commits preserve the
transaction so an authenticated client can abort it.

Ownership is independently checked against the current GUI grants: PID2 owns
Files/window1, PID3 owns Terminal/window2, and neither PID1 nor PID4 may upload
app pixels. Revocation and restart scrub both planes and masks, including
uncommitted bytes. Token counters survive service-session clearing, so a token
from the retired incarnation cannot authorize a later app transaction.

Kernel readiness requires both current app owners, their initial command
metadata, a fully committed plane at the latest metadata revision and no
pending transaction. Publishing later metadata invalidates readiness until
new rows commit; an older complete frame cannot satisfy this condition.

Tests cover guarded layout, individual backing ownership, incomplete/bad rows,
atomic visibility, transparent replacement, abort/replay, revision readiness,
revocation and restart. The existing composition/visibility fixtures now
explicitly invoke the actual pure userspace renderer and these transactions;
production paint has no test or command-raster fallback. No build, test or QEMU
was executed locally. Exact-head CI and graphical/input evidence remain
required before any G3/G4 acceptance claim. This is copied row submission,
not unrestricted zero-copy sharing, GPU acceleration or physical qualification.
