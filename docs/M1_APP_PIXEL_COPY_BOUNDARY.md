# App-private pixel copy boundary

Normal Desktop syscall39 takes a readable64-byte `IPX1` descriptor. The shared
`pixel_protocol` parser defines operation shapes; the runtime independently
authenticates the active PCB, loaded image and live task. Only PID2/Files owns
window1 and PID3/Terminal owns window2. PID1 and PID4 have no upload authority.
No user-selected address is passed to the GUI backend.

REGISTER permanently reserves two guard pages around one private4096-byte
ELF scratch payload. All three pages must be distinct owned frames in one
real writable/NX segment, within its actual memory extent and the same PML4
slot. Code, stack, page-tail slack, aliases and foreign/unmapped frames reject.
The same-PML4 condition ensures unmapping cannot free a root child still
borrowed by the active clone while the middle payload remains live.

Guard mappings are removed from the owned user tables and active translations
invalidated with INVLPG. Their physical frames remain in UserImage's existing
unmapped-awaiting-release state until ordinary process cleanup releases them
exactly once. A partial installation terminates the process; it never resumes
an image with unregistered holes. Repeated registration rejects.

Every later upload and saved-frame resume rechecks the scratch layout, both
absent guards, retained frame ownership and effective user RW/NX payload in
the owned tables and cloned CR3. Mapping validation tolerates zero virtual
entries only for the two registered retained guard frames. The debugger-only
`DESKTOP_APP_PIXEL_GUARDS` ledger publishes `(owner_CR3, lower, payload, upper)`
for each client and is cleared during authority retirement. It is never a
userspace ABI response; user addresses must be inspected under their own CR3.

BEGIN issues a backend-owned transaction. TILE copies at most256 pixels from
the registered payload through existing bounded user-copy helpers into private
kernel storage before touching backend state. COMMIT/ABORT forward only the
authenticated owner, geometry, token and copied pixels. After durable shutdown
arming, only ABORT remains allowed. Kernel READY and shutdown admission also
require both scratch registrations and current, fully committed pixel clients.

Tests were authored for protocol descriptor bounds, invalid/executable/unowned
registration, failed second-guard removal, copy denial and exact cleanup of
retained frames. No local build, tests or QEMU were executed. Fresh exact-head
GitHub acceptance is still required; this is single-BSP copy-based isolation,
not shared-memory GPU rendering or hardware qualification.
