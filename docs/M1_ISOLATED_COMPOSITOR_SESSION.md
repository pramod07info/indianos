# Four-process desktop session and window policy

The `normal-desktop` product profile runs four separately mapped ELF services:
PID1 session/input/file broker, PID2 Files, PID3 Terminal and PID4 compositor.
No service receives a framebuffer address. PID1 alone can poll physical keyboard
and raw bounded pointer packets; it forwards window controls through IPC to PID4.
PID4 alone may query/apply syscall37's canonical64-byte window state. Existing
client focus and legacy frame/action syscalls19/22/23/24 are unavailable in this
profile, so applications cannot bypass the compositor by selecting kernel actions.

Userland PID4 owns focus, stacking, title drag, move, minimize, restore and launcher
policy. It accepts only authenticated PID1 IPC, validates full messages, clamps
geometry to the display and acknowledges the actual selected front window. The
kernel backend validates current app grants, immutable backing extents, canonical
reserved bytes and screen bounds, then paints the proposed state. Failed painting
rolls back state; withdrawn app grants reject later presentation. Kernel glyph
rasterization and retained app drawing remain privileged bounded mechanisms.

Normal startup waits for all services, rather than serial control or fixture
keystroke counts. PID1 has no ring3 presentation authority. Legacy test profiles
retain their original deterministic counters and input fixtures. Normal input
prefers physical ingress even if normal-init was accidentally combined. The
Terminal app cannot consume physical input, and PID4 cannot access file services.

All normal filesystem calls use a boot-admitted selected GPT receipt; normal
syscall32 and34 use the same BSP admission guard as native descriptor calls.
Shutdown first requires a successful selected-volume sync. Failed sync leaves
services running and does not claim successful shutdown. PID4 yields its final
IPC acknowledgement before exiting, because unregistering an owner also purges
outbound messages. The scheduler reclaims all four isolated address spaces.

The pure actual-userland window policy/wire harness passed six tests, including
malformed controls, focus/minimize/restore, bounded title drag and wire checks.
A kernel backend regression covers foreign PID denial, backing resize denial,
withdrawn grants and failed-paint rollback. Full combined compilation and real
QEMU acceptance are coordinator responsibilities after all code is integrated.
These changes do not qualify physical hardware, preemptive multi-core rendering,
general pixel-buffer surfaces, arbitrary app loading or a complete M1 release.

## Named Terminal operations

In the normal desktop, Terminal supports `touch NAME`, `put NAME=VALUE`,
`cat NAME`, `rm NAME`, `mkdir NAME` and `mv FROM TO`. Names are one validated
component beneath `/shared`, never an arbitrary absolute path. PID3 sends the
canonical bounded codec; PID1 independently decodes it and selects an existing
live filesystem syscall. Put overwrites an existing file (touch it first).
The broker determines file/directory rename and removal from a verified directory
listing before issuing one mutation; it never retries ambiguous storage failures.
Empty create and empty read are supported, and read descriptors honor the
72-byte live copy ceiling. The 32-byte editor and 20-byte payload limits remain.

Successful named mutations refresh Files. Normal Files renders a bounded actual
listing with an ellipsis when necessary and reports an unavailable/empty readme
instead of crashing when it is renamed, deleted or emptied. The selected
readme preview remains fixed; this is not a general file-selection GUI. The
legacy scratch-disk profiles retain their old exact fixtures and reject named
operations without entering the new selected-volume broker.
