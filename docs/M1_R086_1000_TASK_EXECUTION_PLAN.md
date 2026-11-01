# IndianOS R086 — 1,200 M1 tasks / 12 proposed integration commits / five workstreams

**Status: 1,200 tasks planned, 0 accepted. A narrow C01 block-read source patch and tests were added; no independent coding agents launched, no QEMU or exact-SHA CI result has been verified yet.**
Verified planning base: main `bf2ef82dd2f17fb9910ab21a48f42e8e1937196e`, merged PR #732.
Priority: [M1 gate tracker #616](https://github.com/pramod07info/indianos/issues/616), `docs/M1_PHASE_SCOPE.md` and `docs/DELIVERY_MILESTONES.md`.

## Contract
- 12 planned source integration commits, C01–C12; each closes 100 checked, independently evidenced engineering tasks.
- Each commit has five explicitly owned logical engineering workstreams, 20 tasks each; 5 agents are a requested deployment configuration, **not currently running**.
- Each stream has five capability slices; each slice has four distinct deliverables: production implementation, live integration, negative/edge tests, and exact-SHA acceptance evidence. Thus 25 capabilities × 4 deliverables = 100 tasks per commit; **250 capability slices × four deliverables plus 100 distinct core-kernel targets / 1,200 total engineering work items**, not 1,000 standalone end-user features.
- Do not mark implementation complete from a text checklist, stub or source-only helper. Require actual production path wiring, tests and evidence.
- First integration commit must address M1 block-backed Terminal/partition/reboot. Do not implement unrelated M2, compatibility, AI or cosmetic changes ahead of required M1 acceptance.
- Check exact SHA using Rust unit/regression/no_std, applicable security, normal boot, real QEMU guest input, disposable-disk remount and recovery; status must come from GitHub, never a guess.
- Fix production failures, do not disable tests, check branch head against expected SHA. Do not merge until all mandatory checks and review pass.
- Keep these twelve task docs as the proposed allocation. Review overlaps and dependencies before implementing any capability, especially if main changes.
- The current docs-only R086 scaffolding commit is **not** C01 completed. Amend/squash this planning commit into the actual C01 integration commit, leaving exactly twelve functional commits once every batch is truly implemented.

## Proposed commit sequence
1. **C01** — Durable Terminal, partition and storage integration. Commit: `feat(m1/r086-c01): integrate durable Terminal and partition-safe QEMU roundtrip (100 tasks)`. [100-item checklist](M1_R086_C01_100_TASKS.md)
2. **C02** — Normal boot, PID1 and service lifecycle. Commit: `feat(m1/r086-c02): wire verified PID1 service graph into normal boot (100 tasks)`. [100-item checklist](M1_R086_C02_100_TASKS.md)
3. **C03** — Independent ring-3 compositor and protected surfaces. Commit: `feat(m1/r086-c03): isolate compositor and user surfaces in ring-3 (100 tasks)`. [100-item checklist](M1_R086_C03_100_TASKS.md)
4. **C04** — Physical keyboard, pointer and desktop interaction. Commit: `feat(m1/r086-c04): route PS2 input to isolated graphical apps (100 tasks)`. [100-item checklist](M1_R086_C04_100_TASKS.md)
5. **C05** — Crash-safe persistent filesystem and recovery. Commit: `feat(m1/r086-c05): harden crash-safe VFS and metadata replay (100 tasks)`. [100-item checklist](M1_R086_C05_100_TASKS.md)
6. **C06** — Authenticated ring-3 filesystem syscall and IPC. Commit: `feat(m1/r086-c06): connect live user syscalls to protected durable VFS (100 tasks)`. [100-item checklist](M1_R086_C06_100_TASKS.md)
7. **C07** — Physical memory, scheduling and process isolation. Commit: `feat(m1/r086-c07): harden process memory and preemptive lifecycle (100 tasks)`. [100-item checklist](M1_R086_C07_100_TASKS.md)
8. **C08** — Essential device drivers, interrupts and shutdown. Commit: `feat(m1/r086-c08): qualify QEMU drivers and safe recovery paths (100 tasks)`. [100-item checklist](M1_R086_C08_100_TASKS.md)
9. **C09** — Security, fault injection and end-to-end reliability. Commit: `test(m1/r086-c09): integrate adversarial security and QEMU reliability gates (100 tasks)`. [100-item checklist](M1_R086_C09_100_TASKS.md)
10. **C10** — M1 exit, release qualification and evidence. Commit: `test(m1/r086-c10): prove complete M1 desktop recovery and exit criteria (100 tasks)`. [100-item checklist](M1_R086_C10_100_TASKS.md)

11. **C11** — Core kernel trap boundary, memory, scheduler, syscall/IPC and fault recovery hardening. Commit: `feat(kernel/r086-c11): harden x86-64 trap boundaries, memory, scheduler, kernel ABI and fault recovery (100 tasks)`. [100-item checklist](M1_R086_C11_100_KERNEL_TASKS.md)

12. **C12** — Firmware, SMP, kernel heap, timekeeping and DMA/device lifecycle. Commit: `feat(kernel/r086-c12): harden firmware, SMP, heap, timekeeping and device lifecycle (100 tasks)`. [100-item checklist](M1_R086_C12_100_KERNEL_TASKS.md)

## Evidence baseline
- PR #732 merged into `main` with R085 improvements; the R085 PR description explicitly distinguishes source operations from the 574 unchecked acceptance items in its expanded checklist.
- The 2026-10-29 M1 roadmap baseline was 5/13 high-level gates with 8 still open; this plan does not mark any gate accepted.
- Existing R085 technical scopes (service graph, IPC, key queue, windows, auth file reads) are prior code; C01–C10 only earn completion when wired to real ingress, durable media and/or QEMU behavior.

## Task ledger
Every task starts unchecked. The 12 linked documents contain 100 numbered tasks each; an item receives a check only with changed code, tests, and exact-SHA evidence.

## Current source delta
The draft PR also hardens `rust/kernel_core/src/block.rs` to prevent failed bounded single-sector reads from changing caller buffers and includes two focused regression tests. This does not satisfy the 100-task C01 completion requirement. See C01 checklist appendix.

## C11 scope accounting
C01–C10 retain their 25 capability slices × four deliverables = 1,000 items. C11 adds 100 individually specified kernel-target work items (20 in each of five independent subsystem scopes) requiring live code, tests and exact-SHA evidence. No C11 tasks have been implemented. The program now plans **11 integration commits with 100 task items each**. Current PR source work remains only the partial C01 block-read guard.

## C12 extension
100 new kernel-specific checklist targets in `M1_R086_C12_100_KERNEL_TASKS.md`; 1,200 planned items total, 12 future implementation commits. None of the 100 C12 items is implemented or verified by this planning update. The historical master-plan filename is retained for stable links.
