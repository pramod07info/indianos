# PID4-only compositor recovery

Scope: G4 / M1-P9-011. The normal desktop scheduler can replace one failed
compositor after the desktop has become ready and before durable shutdown.
Startup failure, shutdown failure, a second compositor failure, failed image
loading and exhausted replacement readiness retain the existing full-session
rollback and bounded fallback policy.

The terminal PID4 process first loses its mailbox, pending inbound/outbound IPC,
continuation and old address space. Only its fully reaped, unmapped PCB may be
reused for the fixed compositor role. The GUI transport retires the old frame
authority and pointer capture before the new image gains authority. Old frame
tokens and epochs cannot authorize the replacement. PID1, Files and Terminal are
neither reloaded nor rebound. The scheduler checks their live image/root and
filesystem-incarnation receipts before and after preparing PID4.

Replacement readiness requires a complete frame published under the new
compositor epoch, followed by PID4's authenticated syscall38 operation2 carrying
that exact epoch. A foreign caller, old/future epoch, duplicate acknowledgment
or acknowledgment before presentation is rejected. Initial desktop readiness
also requires its first compositor acknowledgment. While replacement is pending,
shutdown cannot arm and the ready broker's physical-input calls become captured
zero-result continuations which yield before touching the controller or user
buffer. This preserves the idle broker and lets the replacement run instead of
turning a temporary renderer outage into a fatal input error.

The replacement has at most 256 cooperative dispatches to acknowledge readiness.
This budget is independent of the already-ready desktop state. It does not claim
containment of a task that never traps; that requires the separately qualified
preemptive scheduler. A replacement that withholds readiness consumes the budget
and returns to the existing verified session teardown, rather than retrying an
unbounded number of images.

`DESKTOP_PROCESS_GENERATIONS` records a monotonic successful-image-load generation
for each role. `DESKTOP_CLIENT_INCARNATIONS` records the last successfully loaded
PID1/Files/Terminal generation, CR3 and filesystem epoch. These read-only debugger
receipts are diagnostic evidence, not capability grants. They change when those
clients actually load; reloading only PID4 leaves them unchanged. Live tests also
check guarded app mappings, retained file tokens, graphical state, restored
physical input and durable shutdown; matching a diagnostic record alone is not
acceptance.

Focused host tests exercise the real PCB transition and mailbox implementations,
rejected unreaped/foreign reuse, preservation of the three peers, stale receive
continuations, one-shot epoch readiness, cooperative input deferral and bounded
replacement dispatch. Exact-head QEMU, security, regression, review and merged-main
receipts remain required before formal G4 acceptance.

Cloud verification of this recovery change used a temporary integration checkout
with the frame-transport GUI APIs and syscall37 block overlaid, rather than stub
interfaces. The nine focused G4 tests passed (four recovery tests and five frame
transport tests). All twenty `user_runtime::tests` passed, and the freestanding
`normal-desktop` check passed with existing warnings. The runtime implementation
requires the frame-transport commit before it is independently buildable. These
host results are not live recovery or formal gate-acceptance receipts.
