# G3/G4 app-pixel acceptance extension

This extends the existing normal Desktop acceptance path; it does not add a
second boot, compositor, input or storage implementation. The source audit at
`f7878638e249568e821d74d36c9d5fe7396d4a1e` found automatic four-service boot,
checked recovery, PID4 window policy and kernel-private pixel backing already
implemented. The missing slice is guarded app-owned upload storage and native
ring-3 raster execution through the transactional pixel API.

Verified baseline evidence: [M1 run45023666774](https://github.com/pramod07info/indianos/actions/runs/45023666774),
normal-desktop job137211862171, completed SUCCESS at that exact main revision.
Its log records4186 kernel host tests,38 harness tests,16 profile tests and the
existing compositor/startup/broker tests passing, followed by all25 configured
QEMU boots. Those results establish only the pre-change baseline. PR795 owns
the separate G1/G2 acceptance reconciliation; this work does not duplicate it.

The same25-boot harness now additionally requires fresh, complete per-app
registration, guard-copy denial, authority/transaction denial and real pixel
commit records before each session's kernel READY. PID1 and PID4 must deny a
well-shaped app pixel BEGIN request. Missing, duplicate, stale, partial or
out-of-order evidence fails. Existing exact glyphs, physical focus/capture,
Terminal file readback, UART negative controls and all recovery cases remain.

The existing transient compositor fault test reads both registered app scratch
receipts while paused in actual PID4. It walks the recorded owners' real page
tables using read-only QEMU physical memory inspection, without changing CR3
or treating PID4's virtual mappings as app mappings. Both flanking guards must
be absent; payloads must have effective user RW/NX rights across all ancestors.
App payload physical frames must differ and must not alias at the registered
payload VA in the peer or compositor root. This bounded probe does not scan
every VA; runtime image ownership checks provide the separate frame-ownership
validation. Identical virtual addresses in independent roots are allowed.
The ledger must be cleared after final resource retirement. Kernel retained
pixel backing guard probes now cover both buffers (1188 payload pages/app),
and fixed backing geometry must survive the verified whole-session restart.

These checks complement the backend's transaction/rollback tests and the
actual screenshot oracle. Serial denial records alone do not prove atomic
pixel replacement, an unmapped hardware page, or unchanged visible content.
The standalone native renderer tests are run by the existing normal Desktop
GitHub Actions job; no local compilation, test or QEMU was executed here.

G3/G4 remain IN PROGRESS until the final integrated revision passes required
CI, graphical/memory acceptance and review. This bounded app upload and
privileged scanout interface does not establish a generic shared full-canvas
ABI, fully userspace desktop composition, independent compositor recovery
preserving all client state, hostile CPU availability, SMP or physical-driver
qualification. The accepted milestone baseline is unchanged.
