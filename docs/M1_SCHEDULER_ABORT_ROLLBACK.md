# Scheduler abort rollback and authority retirement

The single-BSP ring-3 scheduler now installs a rollback owner before preparing
its first user context. Preparation failure, mailbox/service initialization
failure, failed dispatch and calibration failure therefore all retire task
credentials and reclaim every initialized context before returning. Normal
completion performs the same final cleanup and reports failed reclamation.

Task retirement cancels pending IPC receives, erases saved register frames,
revokes GUI/input grants, mailbox membership and outbound messages, quantum
jobs, R025 capabilities and generation-bound persistent handles before any
fallible mapping/frame release. Retirement runs once per task incarnation, so
retrying a failed release cannot revoke a replacement process's credentials.

Partial teardown retains its physical ownership metadata. A subsequent
serialized scheduler invocation retries cleanup before zeroing task storage;
it rebinds the trusted direct-map backend context because the previous stack
frame no longer exists. Static tasks clear their stack-local PCB pointers on
all session returns. If CR3 was not restored, authority is retired but potentially
active page tables are retained; re-entry refuses to reclaim the active root.

No new syscall ABI, SMP reclamation, live process-spawn API or general PID1
supervisor is claimed. Full M1 normal desktop acceptance remains open.

The fault-injected runtime regression uses the actual page-table release API:
it denies reclamation, verifies pre-release credential/continuation retirement,
retains ownership, tests a replacement PID mailbox against delayed task cleanup,
then allows exactly one successful release and checks idempotent final cleanup.
Coordinator QEMU process, supervisor and preemption runs remain required on the
combined integration revision.

The scheduler also retains ownership of its APIC timer across aborts. Stop is
attempted before rollback; uncertainty quarantines remaining mappings. Re-entry
must retry and confirm timer stop before reclaiming retained contexts. A pure
ownership regression verifies repeated refusal, successful retry and no duplicate
stop after success. Retry-safe multilevel clone release is supplied by the
companion paging fix documented in M1_PAGING_RELEASE_RETRY.md.

Worker verification: 2,245 host tests passed with PMM/VMM, graphical Files,
supervisor, networking and IRQ-keyboard features. This includes two new runtime
regressions; the three additional paging regressions belong to the independent
paging prerequisite. Bare-metal x86_64-unknown-none release compilation with
preemption enabled also passed. Coordinator QEMU results must be recorded on
their actual tested revisions. No QEMU run was
completed by this worker: the Docker tool call stalled before execution.
