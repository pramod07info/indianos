# IndianOS P0 batch 18 — 40 priority tasks (401–440)

Target existing draft PR #786: https://github.com/pramod07info/indianos/pull/786

**All 40 tasks are OPEN / planned.** This ledger is task planning, not source implementation, six coding agents, CI success, hardware proof, or production acceptance.

These tasks prioritize **real kernel integration** over repeating software-only mock validation. Earlier P0-001–400 and inherited R123 obligations stay separately traceable.

## W1 — SMP / physical APIC / TLB (P0-401–407)

Dependencies: P0-361–367; R123-001–014.

| Task | Implementation obligation | Acceptance evidence | Status |
|---|---|---|---|---|
| P0-401 | Validate MADT-derived CPU and local-APIC topology | Reject duplicate/offline CPU IDs; prove two-vCPU boot | OPEN |
| P0-402 | Wire real AP-side TLB-shootdown interrupt vector | AP handler runs against a dispatched shootdown request in QEMU | OPEN |
| P0-403 | Use actual LAPIC IPI sends with bounded timeout | Failed or partial sends never fabricate AP acknowledgment | OPEN |
| P0-404 | Perform per-AP INVLPG/invalidation of mapped page spans | Instrument each AP and target page before acknowledging | OPEN |
| P0-405 | Fence AP acknowledgment with atomic generation and ordering | Reject stale, duplicate and late completion receipts | OPEN |
| P0-406 | Block real frame reuse until all target APs acknowledge | An unresponsive AP prevents unsafe memory recycling | OPEN |
| P0-407 | Run 2/4-vCPU shootdown, map/unmap and exit stress | Store real QEMU traces and negative-fault logs | OPEN |

## W2 — Ring-3 and scheduler lifecycle (P0-408–414)

Dependencies: P0-368–374 and 395–400; R123-048,062.

| Task | Implementation obligation | Acceptance evidence | Status |
|---|---|---|---|---|
| P0-408 | Install validated ELF PT_LOAD into real page tables | Inspect bytes, BSS, W^X and user/supervisor PTEs | OPEN |
| P0-409 | Create protected user stack and unmapped guard | Guard access faults and legitimate user stack executes | OPEN |
| P0-410 | Enter ring 3 with proper CR3, TSS and interrupt frame | Two isolated executables run without shared writable mappings | OPEN |
| P0-411 | Connect timer preemption to genuine context switching | Two CPU-bound userspace tasks both progress | OPEN |
| P0-412 | Connect exit/fault trap to live resource revocation | Only exact process generation is retired | OPEN |
| P0-413 | Reject recycled-PID races in handle and socket cleanup | Old PID incarnation cannot affect a new process | OPEN |
| P0-414 | Capture full two-process QEMU spawn, exit and fault evidence | Exact-head serial logs and resource reuse proofs | OPEN |

## W3 — Live IPC / cross-core locking (P0-415–421)

Dependencies: P0-375–381; R123-011,027.

| Task | Implementation obligation | Acceptance evidence | Status |
|---|---|---|---|---|
| P0-415 | Bind mailbox permissions to live process capabilities | Unauthorized sender/receiver cannot transact | OPEN |
| P0-416 | Synchronize live IPC queues across AP execution | Two-vCPU concurrent queues have no race or deadlock | OPEN |
| P0-417 | Assign monotonic per-message receipt identity | Identical payloads cannot be confused or replayed | OPEN |
| P0-418 | Validate real ring-3 IPC buffers via live PTEs | Unmapped, guard and short buffers fail without partial reads | OPEN |
| P0-419 | Commit atomic batch IPC on real kernel queue state | Late capacity/copy failure rolls back every mailbox | OPEN |
| P0-420 | Wake blocked IPC receivers and cancel on owner exit | No double-delivery, lost wakeup or stranded waiter | OPEN |
| P0-421 | Run cross-core IPC adversarial QEMU regression | Exercise exit, replay, PID recycle and quota rollback | OPEN |

## W4 — Durable VFS / journal recovery (P0-422–428)

Dependencies: P0-382–388; R123-029–042.

| Task | Implementation obligation | Acceptance evidence | Status |
|---|---|---|---|---|
| P0-422 | Use real block flush/barrier semantics for fsync | Fail flush without falsely acknowledging durability | OPEN |
| P0-423 | Persist journal intent, payload and commit in order | Each injected crash boundary restores valid old or new state | OPEN |
| P0-424 | Validate checksums, sequence and ownership during replay | Torn, truncated or reordered records are rejected | OPEN |
| P0-425 | Make recovery replay idempotent across cold boots | Three restarts preserve file bytes and namespace | OPEN |
| P0-426 | Connect VFS create, delete, rename and fsync to journal | Interrupted transactions preserve namespace invariants | OPEN |
| P0-427 | Validate graphical Files CRUD across two QEMU boots | File SHA-256 and directory contents persist | OPEN |
| P0-428 | Run abrupt QEMU block-power-cut fault matrix | Attach disk images, checksums and recovery logs | OPEN |

## W5 — NIC IRQ / DMA / socket ingress (P0-429–434)

Dependencies: P0-389–394; R123-015–028.

| Task | Implementation obligation | Acceptance evidence | Status |
|---|---|---|---|---|
| P0-429 | Bind real NIC RX descriptors to hardware ring generation | Reject stale completion and foreign-device identity | OPEN |
| P0-430 | Fence PCI/virtio DMA before RX buffer retirement | QEMU driver reset/IOMMU probes detect no unsafe reuse | OPEN |
| P0-431 | Mask, drain and acknowledge IRQ/MSI-X on teardown | Late interrupts cannot deliver retired descriptors | OPEN |
| P0-432 | Unify IRQ/poll ARP, ICMP and UDP ingress fencing | Revoke a packet when ring epoch changes | OPEN |
| P0-433 | Enforce live socket-owner PID generation at delivery | Retired/rebound sockets cannot receive late packets | OPEN |
| P0-434 | Exercise dual-NIC QEMU traffic and reset races | Store packet traces proving isolation and safe quiescence | OPEN |

## W6 — Measured boot / release acceptance (P0-435–440)

Dependencies: P0-395–400; R123-043–070.

| Task | Implementation obligation | Acceptance evidence | Status |
|---|---|---|---|---|
| P0-435 | Authenticate immutable boot public-key trust anchor | Tampered trust source or manifest rejects boot | OPEN |
| P0-436 | Connect kernel/PID1 measurement to TPM/UEFI source | Collect real signed-event or hardware-backed evidence | OPEN |
| P0-437 | Verify installed PID1 PTE digest before actual entry | Refuse entry on wrong bytes, permissions or revision | OPEN |
| P0-438 | Enforce sealed A/B anti-rollback at handoff | Old signed image and interrupted upgrade cannot bypass floor | OPEN |
| P0-439 | Gate exact SHA on all kernel/QEMU/security workflows | No skipped or stale-check acceptance; retain raw artifacts | OPEN |
| P0-440 | Require independent review and protected-branch proof | Log resolved comments, required checks and final main SHA | OPEN |

## Integration and merge criteria

1. Keep all P0-401–440 OPEN until actual Rust source integration and matching nonempty regressions are committed.
2. Retain one consolidated integration commit per implementation batch on PR #786. Avoid placeholder agent source files.
3. Use the existing W1–W6 GitHub Actions **verification jobs**, not pretend six autonomous coding agents are running. Add batch18 test filters only once real tests exist.
4. Prove exact-head freestanding `no_std` and all earlier regressions; require QEMU 2/4-vCPU, ring-3, persistent cold-boot, NIC and secure-boot artifacts as applicable.
5. Physical APIC/INVLPG, DMA/IOMMU, TPM/UEFI and destructive power-loss qualifications remain OPEN until supported by physical evidence.
6. Keep draft; do not merge until independent review, required branch rules and all exact-head checks are verified.

Highest-leverage implementation starts: P0-401–403, P0-408–410, P0-422–424. Later work depends on those foundations.
