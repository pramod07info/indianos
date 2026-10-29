# M1 reliability integration, 2026-10-30 IST

Starting main: `9e69de2146caee4a548fb2af68adf0079dc15599`. Its exact GitHub
kernel workflow completed successfully (135 passing steps; only the optional
shadow-switch dispatch was skipped). Existing PRs 642–646 were already merged
before this batch; none of their completed fixes is duplicated here.

## Accepted functional changes

| Existing production module | Result | Regression evidence |
|---|---|---|
| `vmm.rs` | Failed table release retains reachable ownership; empty-tree cleanup can retry | Refused pruning, map rollback, live sibling preflight and partial cleanup |
| `user_image.rs` | Partial teardown records per-page progress and revokes user-copy access | Data/root release and access failures, invalid page count, no double free |
| `durablefs.rs` | Transport failure propagates as I/O uncertainty rather than false corruption rollback | Both checkpoints and later data-sector faults; no-write repair rejection |
| `m1_partition.rs` | GPT entries are decoded from the same bytes compared and checksummed | Changing backend, altered mirrors, final partial table sector, no writes |
| `m1_persistent_terminal.rs` | Literal payloads and search needles retain trailing spaces | Typed input through durable writes and remount; denied/failed writes |
| `network.rs` | Empty UDP datagrams and owner-scoped per-port close work correctly | Real-checksum ingress, FIFO, short output, stale generation and peer isolation |
| `fs_handles.rs` | Canonical paths are used consistently for authorization and retained handles | Shared/protected aliases, boundaries, token/slot failure atomicity |
| `ipc.rs` | Exiting senders cannot leave stale outbound messages for recycled PIDs | Wrapped/full FIFO enumeration, retained peer payloads and reclaimed capacity |
| `virtio_net.rs` | Device-reported legacy queue sizes determine bounded DMA layout | 256/1024 layouts, last slot/index wrap, actual local NIC packet exchange |
| `input.rs` | CapsLock repeats and corrupt primary transfers preserve valid decoder state | Typematic, modifiers, prefixes, AUX isolation and counter regressions |
| `gui_broker.rs` | Focus/capture authority is bound to the current window grant epoch | Revoke/regrant, reassignment, full queues and epoch exhaustion |
| `hier_vfs.rs` | Canonical listings validate full directory chains and contradictory aliases | Orphans, malformed markers, wrong-type lookups and no-write mutation denial |
| `ata_pio.rs` | IDENTIFY rejects incompatible logical-sector geometry and explicit missing flush | Valid/invalid optional words, 4Kn, incomplete identity and LBA28 bounds |
| Rust/Python smoke runners | Portable HPET disable and bounded QEMU startup diagnostics | Command construction, large stderr, process cleanup and unchanged acceptance |
| `boot.rs` | Framebuffer and module spans cannot wrap or cross canonical address halves | Overflow, alignment, canonical hole and bounded descriptor fixtures |
| `hardware_pcie.rs` | Child and sibling bridge windows cannot route outside their topology | Nested/multifunction mocks, no escaped reads and bus-capacity limits |
| `timer.rs` | Active calibration cannot be reset or sampled after stop | Real production calls on aligned memory-backed register pages |
| `service_supervisor.rs` | Explicit phases prevent pending/exhausted restart activation | Preparation, stop, retry exhaustion, generation overflow and batch rollback |
| `gui.rs` | Hidden/minimized client commands cannot paint over visible peers | Actual memory framebuffer, restore/close, foreign callers and padding |
| `console.rs` | Delayed wrap preserves newline, backspace and bottom-right scrolling | Pixel fixtures, tiny screens and untouched outside-region sentinels |

No new Rust agent-slot files, dependencies, on-disk format or public C ABI
layouts were introduced. Operations retain the documented single-BSP and
serialized-owner requirements. The virtio driver still owns only one submitted
descriptor per queue and keeps the existing DMA-completion fault latch.

## Local validation

The combined host suite, with PMM, VMM, graphical durable Files, supervisor,
network and optional signed-package crypto enabled, passed **2,265 tests** (including IRQ keyboard code).
Freestanding `x86_64-unknown-none` compilation passed with the impacted runtime
features enabled. Existing unrelated compiler warnings were not suppressed.

The original VMM and user-image/initramfs C ABI harnesses passed. Process and
round-robin scheduler QEMU tests passed. Durable VFS passed all five reboot and
corruption stages; graphical Files passed two real boots with actual PS/2 input,
framebuffer changes, persisted data and disk-boundary checks.

The initial local virtio test failed before packet exchange because legacy RX
reported 1024 entries while the driver required 256. After the bounded layout
fix, real QEMU ARP, ICMP, UDP, DNS and TCP exchange passed with host-validated
Ethernet frames. This replaces the earlier unresolved local probe failure with
observed device acceptance. The initial local preemption invocation also failed before boot because this
QEMU removed `-no-hpet`; the revised runners use the equivalent machine property
and retain bounded stderr instead of hiding startup errors. The combined supervisor/guard fault-isolation, R025 IPC/capability/file-syscall,
timer preemption and graphical supervisor-restart QEMU checks also passed.
All 94 Python tests passed. GitHub Actions results are recorded in the PR/report.
Acceptance markers and timeouts are unchanged.

These tests do not close the broad M1 milestone gates. Normal nonexperimental
PID1 startup, a fully independent ring-3 compositor, general per-user durable
permissions, physical-machine qualification and unrestricted sockets remain
unfinished. The milestone ledger is intentionally unchanged.

## Execution accounting and limits

Twenty independent coding agents were requested and **twenty independent
worker sessions started**, across waves with at most six workers alongside the
coordinator. Thirteen workers returned normal final handoffs. One worker hit a
usage-limit error after committing tested code; six early sessions were
interrupted or stalled and their usable work was preserved and finalized by
the coordinator. Execution later became available again, allowing the queue
to reach twenty actual sessions. No unstarted session is counted as launched.

Nineteen workers authored accepted production changes. The coordinator
completed the initially interrupted virtio task, reviewed every diff, corrected
integration issues through worker follow-ups, and assembled twenty meaningful
functional tasks. This is one integration commit/PR, not twenty concurrently
active workers and not completion of the broad M1 milestone gates. No agent
slot files or labels were created to simulate work.

## Highest-priority next twenty tasks

1. Prove live ring-3 Terminal input → selected partition → reboot → GUI readback.
2. Inject power cuts at every durable metadata/data publication boundary.
3. Bind all filesystem syscall capabilities to scheduler PID and generation.
4. Define stable inode/handle behavior across rename, unlink and recreation.
5. Enforce serialized volume ownership and explicit flush/fsync guarantees.
6. Launch authenticated PID1 from normal boot without experimental flags.
7. Add deterministic recovery boot for missing/corrupt init and storage.
8. Move compositor stacking and lifecycle into an independent ring-3 service.
9. Move input routing into its separately isolated authenticated service.
10. Define app-surface shared memory and event backpressure across process death.
11. Stress GUI service crash/restart without losing durable data or permissions.
12. Integrate grant generations into every live compositor/window lifecycle route.
13. Stress mixed keyboard/mouse IRQ traffic with actual QEMU events.
14. Add bounded blocking IPC/wakeups with authenticated process teardown.
15. Boot signed allowlisted service manifests with deterministic missing-app recovery.
16. Verify clean ATA flush, shutdown and restart on disposable QEMU disks.
17. Stress multi-process durable file syscalls across genuine reboots.
18. Revalidate pinned volume identity across media replacement and recovery.
19. Fault-inject OOM through process startup, UI restart and late frame reclamation.
20. Publish a precise qualified QEMU driver/boot matrix before physical testing.

These are prioritized candidates; inspect the latest main and CI again before
assigning them. Do not start another integration batch while this batch has
unresolved mandatory checks.
