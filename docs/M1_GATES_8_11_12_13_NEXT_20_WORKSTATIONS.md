# M1 next 20 kernel-first tasks — PR #784 continuation

Date: 2026-11-17. Parent head: `4e7ceb167fa71a7865436560dae146ff23c80f73`.
Branch: `feat/m1-gates-6-13-90-task-kernel-ledger-20261116`. Planned as **one new consolidated source commit**.
No agent coding workstations were launched; these are six independent GitHub
Actions verification workstations. Gate acceptance remains open until full
exact-head QEMU/freestanding/hardware and independent review evidence.

| Verification workstation | Gate | New master IDs | Actual implementation slice | Remaining requirements |
| --- | --- | --- | --- | --- |
| W1 boot | 8 | 009, 010, 012, 013 | Entire memory map preflight at region export; canonical/aligned high-half direct-map numeric guard; negative overflow tests | Pinned Limine, ELF loading, real boot recovery and serial/framebuffer guest proof |
| W2 PID1 | 8 | 018, 019, 021, 022, 023 | Transactional dependency-ordered multi-service start, stale/cyclic batch rejection, exact PID/generation preflight | Actual PID1 startup and watchdog/failover/logger daemon integration |
| W3 persistent FS | 11 | 060 | Authenticated per-file configurable quota on create/write, no COW mutation on quota failure, cold remount regression | Durable per-UID/group volume quotas, free-space allocator and powercut tests |
| W4 IPC | 12 | 064, 065, 066 | Exact-generation atomic prefix receive with failed-copy rollback and bounded capacity checks | Ring-3 syscall, signal cancellation and kernel wait queue/futex scheduling |
| W5 scheduler/SMP | 13 | 072, 073, 074, 075, 076 | Priority-threshold dispatch with low-priority fallback; multi-AP exact receipt memory-reclamation barrier | Real paging/PMM/TLB shootdown, APIC timer, 2-vCPU scheduler and lock qualification |
| W6 firmware/MMIO | 13 | 082, 084 | Whole-set MCFG/ECAM aperture and reserved-physical-range checks before numeric config address | Trusted ACPI mapping, MMIO backend, devices and hardware qualification |

**20 distinct selected IDs:** 009, 010, 012, 013, 018, 019, 021, 022, 023,
060, 064, 065, 066, 072, 073, 074, 075, 076, 082, 084.

All are **targeted/in progress**, not accepted. Some source changes are
guarded helpers and cannot be called full runtime implementations. The
original 90 master checkboxes remain unchecked until evidence establishes
end-to-end acceptance. CI runs the existing W1–W6 filters, including new
`m1_gate20_w*_next` regressions. Do not merge the draft PR without review.
