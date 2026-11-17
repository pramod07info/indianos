# M1 Gates 6–13: batch 01 — first 20 tasks, six scoped workstreams
Date: 2026-11-17. Target: existing draft PR #784, branch `feat/m1-gates-6-13-90-task-kernel-ledger-20261116`.
Source of truth: `docs/M1_GATES_6_13_TASKS_001_090_WORKSTREAMS.md`.
This is an **implementation and verification batch**, not a claim of 20 accepted master gates, six independent processes or 90 completed tasks.

## Six workstation ownership plans (20 master tasks, still OPEN)
| Workstream | Master task IDs | Engineering scope for this batch | Acceptance evidence still needed |
|---|---|---|---|
| W1 File operations and desktop | 001–004 | Fail-closed canonical rename and live HierVfs directory preflight; regression cases B01-01..04 | Real Files CRUD/Terminal/editor on normal QEMU Desktop, persistence and permissions |
| W2 Partition and disk | 005–007 | Block window and LBA boundary regressions B01-05..07 | GPT/MBR safety, ATA hardware integration, removable media identities |
| W3 Boot and memory protocol | 008–011 | Shared invalid-region guard for boot_validate and exported region API; refuse invalid map counts; B01-08..11 | UEFI/ELF/Limine boot and full normal-QEMU+firmware traces |
| W4 Startup reliability | 012–015 | Host-only service restart and exact PID generation regressions B01-15..16 (workstream cross-check for boot/recovery) | Fault/recovery normal boot and validated initramfs, framebuffer, boot options |
| W5 Process identity and ACL | 016–018 | Owner and ancestor ACL assertions B01-12..14 | Ring3 PCB isolation, process reaping and parent/child policy integration |
| W6 Normal desktop + PID1 | 019–020 | Peer identity state-hardening and checks B01-17..20 as supporting boot network boundary | Normal desktop start, authenticated PID1 supervision, independent review |

Task IDs are stable **master task IDs**; B01-01..B01-20 are 20 narrow **host regression items**, not 20 completed master tasks.
The six workstreams are ownership partitions for this integration, **not** six launched autonomous agents.

## Source changes
- `vfs_path`: canonical parent-to-descendant directory rename guard; `hier_vfs`: fail before selected-volume directory-rename I/O.
- `boot`: validate memory region before returning it to C and reject invalid map counts.
- `batch3_network_security`: refuse multicast/broadcast BOOTP client MAC, refuse non-unicast peer IP in TCP SYN policy.
- `block`, `fs_permissions`, `service_supervisor`: boundary, permission, and PID-generation regression tests.

## Verification
- 20 `#[test]` regressions are provided, but do not mark them PASS without CI at the final commit SHA.
- Require freestanding kernel/no_std build, kernel-core host tests, security checks, normal QEMU desktop boot and cold remount artifact.
- Do not merge or mark the master 90 ledger items complete without that evidence.
- Physical devices, power loss, NIC IRQ/DMA/IOMMU and protected independent review remain OPEN.
