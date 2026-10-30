# M1 runtime and recovery integration

Starting main: `8479fed35156dadfbec98f9d2b507e06c9de4a84`. The exact-main
kernel workflow completed successfully (135 successful steps, no failures).
PRs 652 and 654 already supplied GPT-bound Terminal session APIs; this batch
does not duplicate those changes or mark the broad M1 milestone gates complete.

## Functional deliverables

| Existing subsystem | Production behavior | Evidence |
|---|---|---|
| DurableFs | Rejected checkpoints retain bank ownership; changed geometry rejects mutations | Baseline resurrection reproduced, 415 cut cases, capacity-change regression and independent public-API recheck |
| Runtime file handles | Handles bind to monotonic process incarnations; stale cleanup cannot revoke replacements | Stale operations, legacy isolation, exhausted generations and runtime compilation |
| Selected GPT recovery | Explicit authenticated repair keeps GUID/geometry binding and returns a fresh healthy session | 513 partial metadata writes, denied callers without I/O, neighbor guards and retired-session isolation |
| IPC and scheduler | Syscall 35 waits for bounded cooperative rounds, copies before dequeue, and cancels on teardown | FIFO/copy failure, mailbox epochs, all-waiting progress and IRQ/preempt continuation tests |
| Graphical Terminal | Actual PID3 editing uses a PID1 broker for the durable shared note; Files refreshes and reboot readback works | Two real QEMU boots, physical PS/2 events, exact authenticated readback, actual pixels and PCB cleanup |
| Init recovery | Missing/invalid init enters a read-only bounded COM1 console | Normal control plus four damaged ISO boots, actual UART commands and unchanged writable sentinel disk |

The coordinator also restored physical Tab delivery at the input filter, which
previously discarded the already-mapped key. Drawing operation 3 replaces only
an existing owned text field with the same rectangle; it does not consume more
retained command slots. The Files-GUI profile has a finite 256-dispatch budget
for per-key service exchanges, while legacy32/preempt2048 and frame64 bounds
remain. These are experimental profiles, not production normal desktop startup.

## Validation and review

The integrated host/security suite passed 2,325 tests; 107 Python regressions
and four real userland parser tests passed. Combined freestanding compilation,
secure IPC/file-syscall QEMU, APIC preemption and physical IRQ keyboard QEMU
passed. The new Terminal persistence and init recovery QEMU acceptances passed.
Existing two-boot graphical Files, supervisor restart and five-stage durable
VFS QEMU regressions also passed. Exact-SHA GitHub Actions results are recorded
in the PR/execution report.

Independent review found and corrected two integration defects: changing device
capacity could bypass the first quarantine fix, and IRQ-keyboard continuations
used an inconsistent IF policy. No privilege checks were weakened. Local test
setup failures included disk exhaustion and read-only/existing hybrid ISO
fixtures; generated old caches were cleaned and fixtures rebuilt without
ignoring failures. Source, commits and raw test logs were retained.

Six genuine coding worker sessions ran, with at most six workers plus the
coordinator. Four returned completed code handoffs; two tool executions stalled
and their saved source was finalized and tested by the coordinator. All six
workers authored accepted production changes. The standing target of twenty
coding agents was not reached in this batch; fourteen remain queued. Earlier
read-only inspection sessions are not counted as coding deliverables.

One integration commit/PR packages the accepted work. Merge requires applicable
GitHub checks to pass. No new agent-slot files, dependencies, disk formats or
public Process/Scheduler/wire layouts were introduced.

## Limits and next priorities

Terminal currently brokers only `/shared/readme` on the qualified disposable
volume. Normal PID1, live GPT-selected storage, unrestricted shell commands,
general durable ACLs, physical hardware qualification and an independent
userspace compositor remain unfinished. IPC deadlines count cooperative
selection rounds rather than hardware time. Block operations require a
serialized/quiescent owner; geometry checks do not provide an SMP device lock.
Boot recovery cannot retry uncertain staging or repair storage.

1. Connect the live Terminal broker to explicitly selected GPT sessions.
2. Add authenticated path-based Terminal operations beyond the shared note.
3. Start a practical trusted PID1 during normal boot.
4. Add deterministic read-only storage failure diagnostics to recovery boot.
5. Replace bounded preview sessions with a supervised production lifecycle.
6. Move compositor stacking/lifecycle into an independent ring-3 service.
7. Isolate input routing as its own authenticated service.
8. Define file-object/handle identity across rename, unlink and recreation.
9. Enforce serialized volume ownership and explicit flush/fsync guarantees.
10. Inject actual QEMU power cuts during ATA publication and remount.
11. Stress mixed PS/2 IRQ input together with live IPC waits.
12. Add hardware-clock IPC deadlines with idle/wakeup handling.
13. Integrate process cancellation and immediate peer-death wait notifications.
14. Fault-inject OOM through process preparation and all abort cleanup paths.
15. Boot signed allowlisted service manifests with missing-app recovery.
16. Implement bounded shared app surfaces and event backpressure.
17. Persist user/group ownership and enforce filesystem ACLs.
18. Verify ATA flush, clean shutdown and restart on disposable QEMU media.
19. Extend failed-copy atomicity to runtime file cursor/publication boundaries.
20. Publish an exact-SHA qualified QEMU driver matrix before physical testing.

Reassess latest main before scheduling these; do not start a new feature batch
while mandatory integration failures remain unresolved.
