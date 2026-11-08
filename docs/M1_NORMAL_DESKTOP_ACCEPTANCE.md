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

The G1 Terminal contract also creates a named file, writes `violet`, renames it,
deletes another file and creates a directory through the actual ring-3 broker.
Every structural mutation must change visible Files pixels and produce the exact
expected directory glyphs. After cold reboot, Files must show the persisted
directory and note; a freshly typed named `cat` must render exact `violet` glyphs.
The payload deliberately differs from the status label `SAVED`, since the font
renders lowercase in uppercase and a `saved` payload would be visually ambiguous.
Exact serial content and exact framebuffer text are independent requirements.

The raster oracle pins the qualified normal window geometry and 5x7 doubled font
contract independently of the production renderer. It checks every foreground
and background/spacing pixel in the bounded field, including cleared trailing
cells. Stale success labels, wrong payloads, prefix-only matches, leftover text
or output in another window cannot satisfy it. Normal acceptance performs no
window moves/resizes; an unqualified geometry fails rather than searching the
screen for a convenient matching string. Fresh physical key acknowledgements
precede result screenshots. Missing pre-readiness negative syscall controls from
Files/Terminal/broker/compositor also fail the normal runtime verifier.

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
or disk modification. The configured matrix now comprises twenty-two actual VM
boots, including the partition and write-failure cases and the two lifecycle
failures below. Configuring these checks does not establish a passing run.

Normal startup additionally requires the kernel's PID1 lifecycle-ready handshake
after the service and authority checks. Clean shutdown requires the kernel's
durable-shutdown handshake before the userland synchronization acknowledgment;
service exit status zero alone cannot authorize shutdown. Two disposable,
otherwise valid compositor ELFs exercise this distinction: one immediately
invokes the real exit syscall with status zero, and one invokes the real yield
syscall forever without becoming ready. Each must trigger verified teardown,
one bounded restart, a second verified teardown and read-only recovery, with no
desktop READY or durable-shutdown acknowledgment and no disk changes. The latter
tests the 256-dispatch cooperative startup budget, not a CPU-hog watchdog.

The transient recovery debugger also reads the production
`DESKTOP_SURFACE_LAYOUT` ownership ledger from the exact kernel ELF. It verifies
both private one-page surface payloads are readable and all four flanking guard
pages are unreadable under PID4's cloned root, then repeats those probes under
the reclaimed recovery-kernel root and requires unchanged ownership geometry.
These probes inspect real page mappings without writing page tables or payloads.
Host negative controls reject malformed/overlapping ledgers, readable guards,
missing payload mappings, premature readiness and incomplete lifecycle cleanup.

The harness source and its negative controls must run in GitHub Actions at the
integrated revision before these new criteria can be accepted. No local QEMU or
test execution is implied by this update.

The normal two-boot path also keeps a `cat` command pending in Terminal, clicks
Files through the emulated PS/2 mouse, and sends a physical `z`. It requires a
fresh suspension acknowledgment, no Terminal input acknowledgment and an
unchanged framebuffer. A physical click back into Terminal must change focus
visually; Enter must read the exact original file contents both in serial and
in rendered glyphs. This detects accidental keyboard delivery to an unfocused
editor and proves that its pending command survives focus changes. Mouse motion
uses relative deltas from the known initial pointer, not absolute HMP arguments.

Healthy lifecycle evidence now requires each readiness/shutdown transition
exactly once as a complete record, compositor startup before kernel readiness,
and every real process exit followed by its reap after durable shutdown arming
and before final session reclamation. The single physical console echo before
the arm acknowledgment is accepted explicitly; arbitrary containing diagnostics
or partial/replayed records cannot satisfy this evidence.

Verified upstream result for `6b261b5a16b963fec89fadcc668ee164b395b418`:
[M1 run 42510705119](https://github.com/pramod07info/indianos/actions/runs/42510705119)
passed combined regressions and the runtime QEMU job. The normal-desktop job
passed its host/build checks and the healthy boots, reboot, transient guard/fault
recovery, media rejection, write-failure, invalid service, repeated fault and
premature zero-exit scenarios. It failed the final unready-compositor criterion:
the retained guest serial log shows PID1 exiting with status 142 before the
startup dispatch budget, because its startup receive treated an absent compositor
handshake as an immediate error. The blocking-startup-receive correction and
new pointer/lifecycle checks require a fresh exact-revision CI run; this result
does not close either high-level gate.
See `M1_NAMESPACE_ADMISSION.md` and `M1_KERNEL_STACK_GUARDS.md` for limits.

Run the combined normal-desktop workflow only after source integration. No new
QEMU result is claimed merely by adding these scenarios. Physical hardware,
independent IRQ services, general ACLs/locking and hostile physical power-loss
qualification remain outside this bounded scenario. Gates can be
accepted only after their exact runtime exit criteria pass at the final source
revision; this document alone closes none.
