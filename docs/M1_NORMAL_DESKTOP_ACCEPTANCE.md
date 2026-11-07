# Normal desktop acceptance on explicitly selected GPT media

The new `tools/test_normal_desktop_qemu.py` targets the clean
`normal-desktop` product profile, not a renamed Files-GUI test feature. The
host-only `indianos-desktop-fixture` tool provisions a **new** disposable GPT
image with the actual kernel HierVfs/COW implementation and emits an explicit
64-byte `INDVOL01` selection. The guest neither discovers an arbitrary writable
volume nor formats the disk. Original fixture files are copied before QEMU use.

The acceptance requires all four actual process READY/DISPATCHED transitions,
PID4 compositor publication before the desktop ready marker, real visible
Files/Terminal windows, and physical PS/2 launcher/focus/mouse/editor actions.
UART `qaa21` injection is a negative control: no desktop action, input provenance
or framebuffer change may result. Subsequent accepted actions come exclusively
from QEMU's emulated keyboard/mouse, with fresh responses and actual screenshots.

Two cold QEMU/OVMF boots reuse the same disposable selected partition. The first
types and saves the note, requiring visible Files repaint. The second reads the
exact committed bytes through Terminal. Both read real directory entries and
note size, then require a successful storage barrier before service shutdown and
all four PCB reaps. Read-only commands on the second boot may not mutate media.
Neighbor sectors, GPT metadata and every byte outside the selected partition
must remain unchanged throughout both boots.

Negative guests receive a corrupt GPT checksum, validly checksummed changed disk
GUID, or two corrupt COW metadata heads. Each must enter read-only recovery
without process startup, formatting or any disk change. Fresh ISO fault fixtures
also remove or invalidate the compositor ELF and invalidate the selection module;
these must fail before desktop startup and leave writable sentinel storage intact.
The original initramfs entries remain untouched except the explicit fault target.

Each invocation retains fresh serial logs and actual PPM screenshots in a new
artifact subdirectory. CI uploads those artifacts even when acceptance fails.
Host negative controls reject stale/missing runtime evidence, wrong barrier
ordering, fake UART actions, guard writes and malformed fault source archives.
These are harness checks, **not guest execution evidence**.

An additional two-boot scenario saves a named file through the physical Terminal,
requires its commit acknowledgement, then sends SIGKILL to QEMU without the guest
shutdown/sync command. A fresh boot must read the exact saved bytes through new
ring-3 services, with unchanged neighboring sectors and no readback writes.
This covers abrupt guest loss after an acknowledged commit while host storage
remains intact. It does not model a torn ATA write or physical power failure.

The compositor-fault image must produce two actual PID4 faults and reaps, two
verified cleanup markers, exactly one session restart and then exhausted-budget
read-only recovery. Missing, reordered or extra lifecycle evidence fails the
harness. The deterministic bad executable may never publish desktop readiness.

A separate transient-fault boot starts from the unchanged production image and
waits for its real graphical desktop. A local UNIX GDB stub resolves symbols
from the exact kernel and compositor ELFs extracted from that ISO. A temporary
execution breakpoint identifies actual PID4 in ring3 at known executable bytes;
the harness then removes the breakpoint and changes only its user RIP to a
verified unmapped address. No code, disk data, page tables, kernel memory or
production fault-injection hook is modified. Physical input is sent after VM
resume so a paused VM cannot lose the trigger before the breakpoint executes.

The guest must report the actual instruction-fetch page fault, reap PID4, verify
retirement and restart exactly once. All four fresh processes must execute, the
new compositor must independently fail forbidden input/filesystem probes, and
the new desktop must become ready. Actual PS/2 launcher/focus/editor input then
creates, saves and reads a named file through the restarted services. The normal
durable barrier and all four clean service reaps are required at shutdown, with
no change outside the selected partition. A second legitimate PID4 reap belongs
to clean shutdown; it is counted separately from the faulted incarnation's reap.
New logs and screenshots include the debugger's verified PID/CS/RIP and original
code bytes. Negative controls reject missing, stale, duplicated and reordered
fault/restart/readiness evidence and malformed debugger packets or ELF ranges.

This is an emulator debugger fault test, not evidence of hardware fault recovery,
arbitrary application availability or persistent ACLs. Kernel generation/handle
tests supply the separate stale-incarnation authority evidence; fresh app
execution and commands alone do not prove every stale handle was exercised.

The same debugger session now verifies all five kernel trap/DF guard pages are
unreadable while their usable endpoints remain readable, first under PID4's
cloned root and later under a distinct recovery-kernel CR3 after reclamation.
Bounds come from sized stack symbols in the exact booted ELF. This is mapping
evidence; it does not inject a kernel stack overflow or qualify DF delivery.

Four additional boots use healthy-checksum COW images containing orphan entries,
contradictory aliases, malformed directory markers or invalid symlink targets.
The real COW fixture writer generates each explicitly; no secondary checksum
encoder is used. Every image must fail storage admission with no service startup
or disk modification. The complete matrix now comprises sixteen actual VM boots.
See `M1_NAMESPACE_ADMISSION.md` and `M1_KERNEL_STACK_GUARDS.md` for limits.

Run the combined normal-desktop workflow only after source integration. No new
QEMU result is claimed merely by adding these scenarios. Physical hardware,
independent IRQ services, general ACLs/locking and hostile physical power-loss
qualification remain outside this bounded scenario. Gates can be
accepted only after their exact runtime exit criteria pass at the final source
revision; this document alone closes none.
