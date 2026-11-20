# IndianOS kernel-first P0 batch 19 — next 40 priority tasks (441–480)

Target: existing draft [PR #786](https://github.com/pramod07info/indianos/pull/786), branch `feat/p0-kernel-six-workstations-20-20261117`.

**All 40 items: OPEN / PLANNED.** This commit adds the task ledger; it does not implement source code, run six coding agents, prove CI results, or qualify production hardware.

## Why this batch

This batch builds on the integration obligations in P0-401–440 without treating planned integration as finished. It prioritizes kernel memory correctness, scheduler progress, syscall/userland security, crash-safe persistence, real network egress and a usable normal-boot desktop. A new task may depend on a prior task that remains OPEN: do not mark the dependent task complete first.

## Task ledger and acceptance criteria

### W1 — Physical memory, page faults and SMP correctness

Dependencies: P0-401–407, previous APIC/TLB lifecycle; R123-001–014.

| ID | Distinct implementation task | Objective acceptance evidence | State |
|---|---|---|---|
| P0-441 | Detect double-free and wrong-owner physical frame retirement | Fault-injected reclaim must preserve allocator free-list and reject stale owner generation | OPEN |
| P0-442 | Implement generation-checked address-space frame ownership ledger | PID reuse and ASID reuse cannot inherit another process’s retired mappings | OPEN |
| P0-443 | Audit cross-CPU allocator locking and memory ordering | Two/four-vCPU concurrent alloc/free/unmap test reports zero corrupted free lists | OPEN |
| P0-444 | Handle kernel and user page faults with correct privilege separation | Bad user address kills the owning process; kernel integrity remains intact | OPEN |
| P0-445 | Add bounded stack-overflow and guard-page fault diagnostics | User/kernel guard trips produce correct fault source and no silent reuse | OPEN |
| P0-446 | Define fail-closed physical memory exhaustion and recovery | Out-of-memory stress returns explicit errors without corrupting live mappings | OPEN |
| P0-447 | Run page-allocator and TLB-reclaim chaos tests on 2/4-vCPU QEMU | Retain exact-SHA traces with injected interleavings, cleanup invariants and timeouts | OPEN |

### W2 — Scheduler, timers and process supervision

Dependencies: P0-408–414, ring-3 entry and timer-driven scheduling.

| ID | Distinct implementation task | Objective acceptance evidence | State |
|---|---|---|---|
| P0-448 | Calibrate monotonic scheduler clock against QEMU timer source | Clock never moves backwards; timer wrap/drift tests are bounded | OPEN |
| P0-449 | Implement sleep/wakeup timer queue with generation-safe cancellation | Canceled/recycled sleepers cannot wake a different PID incarnation | OPEN |
| P0-450 | Eliminate lost wakeups during concurrent wait/notify transitions | Two-vCPU blocking and exit races preserve exactly-one runnable transition | OPEN |
| P0-451 | Enforce bounded priority inversion handling for kernel lock holders | High-priority runnable work does not starve behind an unbounded lock owner | OPEN |
| P0-452 | Apply CPU affinity and runqueue migration under real locks | Concurrent migration, exit and CPU-offline simulation preserve one owner queue | OPEN |
| P0-453 | Implement supervisor policy for crashed user services with backoff | Repeated PID1 child failure does not cause tight restart loops or lost exit reports | OPEN |
| P0-454 | Run multi-process latency, fairness and kill/restart QEMU suite | Record timer latency, progress counters and no duplicate runnable entries | OPEN |

### W3 — Syscalls, capabilities and real userland I/O

Dependencies: P0-410–420 (ring3, IPC and capability checks).

| ID | Distinct implementation task | Objective acceptance evidence | State |
|---|---|---|---|
| P0-455 | Version and validate x86-64 syscall calling convention | Arguments, error codes and register preservation pass ring-3 test programs | OPEN |
| P0-456 | Implement safe copyin/copyout with checked user pointer ranges | Cross-page, null, unmapped, overflow and read-only buffers fail cleanly | OPEN |
| P0-457 | Enforce generation-tagged capabilities for file and IPC handles | Forged, closed or recycled handles cannot access another process | OPEN |
| P0-458 | Add bounded blocking IPC/syscall timeout and interruption semantics | Timeout and signal/exit are distinguishable with no orphaned messages | OPEN |
| P0-459 | Enforce per-process limits for descriptors, messages and byte quotas | Resource exhaustion cannot deny all other processes kernel service | OPEN |
| P0-460 | Connect terminal commands to real process, IPC and VFS syscalls | Ring-3 shell can create, read and rename files through permission checks | OPEN |
| P0-461 | Run malicious ring-3 syscall fuzz and capability isolation tests | Capture invalid-pointer, handle-forgery, quota and cancellation regressions | OPEN |

### W4 — Filesystem namespace, recovery and durability

Dependencies: P0-422–428 (block barriers, journal and cold-boot tests).

| ID | Distinct implementation task | Objective acceptance evidence | State |
|---|---|---|---|
| P0-462 | Serialize live mount revision lookup across CPUs | A concurrent remount cannot create a stale-root path-to-handle authority | OPEN |
| P0-463 | Bind file handles to immutable mounted-device identity and PID generation | Device swap and recycled PID invalidate stale handles without data leakage | OPEN |
| P0-464 | Make directory rename/unlink atomic with journal recovery | Crash during nested rename cannot leave cycles or orphan published entries | OPEN |
| P0-465 | Define ENOSPC and partial-block-write rollback for metadata operations | Disk-full and short-write faults do not acknowledge incomplete transactions | OPEN |
| P0-466 | Validate fsync guarantees across metadata, directory and data writes | Acked changes survive cold reboot; unacked changes recover consistently | OPEN |
| P0-467 | Reject wrong-disk and stale-superblock replay on subsequent boots | Old revision and swapped partition cannot replay into another volume | OPEN |
| P0-468 | Automate graphical Files two-cold-boot integrity under interrupted I/O | Capture filenames, content hashes and guest logs across crash/reboot cycles | OPEN |

### W5 — Network egress and resilient guest connectivity

Dependencies: P0-429–434 (NIC DMA/IRQ and socket ownership).

| ID | Distinct implementation task | Objective acceptance evidence | State |
|---|---|---|---|
| P0-469 | Implement generation-checked virtio-net TX completion and buffer ownership | Device reset and late TX completion never free a live or foreign buffer | OPEN |
| P0-470 | Enforce neighbor-cache ARP expiry and authenticated replacement policy | Spoofed/unexpected gateway replies do not overwrite trusted live peer | OPEN |
| P0-471 | Connect user-owned UDP send/receive to actual NIC queues | Two isolated ring-3 processes exchange datagrams without crossing socket ownership | OPEN |
| P0-472 | Implement bounded DNS request correlation, retry and timeouts | Stale transaction IDs, truncation and malformed replies cannot satisfy current query | OPEN |
| P0-473 | Recover network interfaces after link flap and virtio ring reset | Rebinding revokes old RX/TX handles while reconnecting new socket generation | OPEN |
| P0-474 | Run dual-NIC QEMU network tests with packet loss, reset and PID recycling | Collect pcap/serial artifacts proving isolation and bounded recovery | OPEN |

### W6 — Normal boot, desktop continuity and release evidence

Dependencies: P0-435–440 (boot trust and release gates), plus W2/W3/W4 integrations.

| ID | Distinct implementation task | Objective acceptance evidence | State |
|---|---|---|---|
| P0-475 | Validate actual bootloader memory-map reservations across handoff | Reject overlapping reserved/runtime spans before allocator and userspace setup | OPEN |
| P0-476 | Provide signed recovery-slot fallback on verified PID1/boot failure | Failure path selects only verified recovery image and preserves user data | OPEN |
| P0-477 | Wire service dependency graph for desktop PID1 startup | Normal boot launches required services exactly once and reports failed dependencies | OPEN |
| P0-478 | Route keyboard input and window focus into graphical Terminal and Files | QEMU input events reach correct ring-3 client without foreign-process leakage | OPEN |
| P0-479 | Persist crash diagnostics with bounded serial and on-disk error evidence | Panic/exception records include CPU, fault origin, boot epoch and failing stage | OPEN |
| P0-480 | Enforce exact-head normal-boot-to-desktop, reboot and recovery CI gate | Build exact SHA; QEMU desktop usable, data durable, recovery tested; no skipped gates | OPEN |

## Priority execution and evidence checklist

**P0 foundation first**: close the prerequisites in P0-401–440 for real APIC/TLB, ring-3, IPC, durable writes, NIC ownership and verified handoff. Start this batch with W1 frame-owner integrity, W2 scheduler clock/wakeup, W3 syscall ABI/copyin, and W4 mount-revision synchronization in parallel only when their prerequisite interfaces exist.

- Each item must have actual production Rust source changes, targeted negative tests and exact file/line evidence before moving from OPEN to IMPLEMENTED.
- Tests in the corresponding six GitHub Actions W1–W6 verification lanes should include `kernel_p0_batch19_w1` through `kernel_p0_batch19_w6` **only after** the relevant source/test cases are introduced. No fake passing filters or placeholder modules.
- A completed task must carry exact commit SHA, non-skipped test name, observed result, QEMU serial log (when applicable), and a documented remaining physical-hardware gap.
- Require a full freestanding x86_64 `no_std` build and full inherited P0 regression matrix on the exact PR head, plus kernel boot, multi-vCPU, ring-3, desktop, network and persistent cold-boot tests before any release claims.
- Do not conflate software-model or QEMU simulation with physical APIC ordering, DMA/IOMMU, TPM/UEFI attestation or destructive physical power-loss qualification.
- Preserve existing PR as **draft**. No merge until independent review, all mandatory exact-head checks, and actual protected-main policy enforcement are verified.

## Batch summary

| Workstation | P0 IDs | Task count |
|---|---|---:|
| W1 | 441–447 | 7 |
| W2 | 448–454 | 7 |
| W3 | 455–461 | 7 |
| W4 | 462–468 | 7 |
| W5 | 469–474 | 6 |
| W6 | 475–480 | 6 |
| **Total** | **441–480** | **40** |

**Note:** 480 numbered P0 ledger entries are not 480 implemented or accepted features. Completion statistics must be evidence-based.
