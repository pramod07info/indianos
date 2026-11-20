# IndianOS kernel P0 batch 9 — P0-161 through P0-180

Continuation in draft PR #786, starting from `6009d13f7b5956734ee4d9a2ecbb144332983216`. The twenty IDs below are source implementation and regression obligations, **not** full hardware or production acceptance. W1–W6 are six **GitHub Actions validation jobs**, not six autonomous coding agents.

| ID | Workstation | Source-level scope and acceptance |
|---|---|---|
| P0-161 | W1 | Publish trusted multi-AP topology in one exact epoch |
| P0-162 | W1 | Expose each published receipt before real IPI send attempt |
| P0-163 | W1 | Never hide partially sent IPI receipts or permit premature reclaim |
| P0-164 | W1 | Reject duplicate APs, forged BSP domain, and reused output buffer |
| P0-165 | W2 | Atomic scheduler retirement + process-exit event + IPC mailbox cleanup |
| P0-166 | W2 | Purge queued outgoing messages from the retiring generation |
| P0-167 | W2 | Roll back all three structures on stale IPC owner generation |
| P0-168 | W2 | Roll back all structures on full event queue or failed AP proof |
| P0-169 | W3 | Authenticate sender generations on incoming FIFO prefix before copy |
| P0-170 | W3 | Enforce aggregate receive-byte quota |
| P0-171 | W3 | Reject stale, unlisted, duplicate PID authorization without dequeue |
| P0-172 | W3 | Preserve FIFO on failed user-copy; commit on success |
| P0-173 | W4 | Domain-bound SHA-256 digest for partition identity and field position |
| P0-174 | W4 | Paired stable verified reads with all-or-none output publication |
| P0-175 | W4 | Permanently retire disk view on foreign-partition digest mismatch |
| P0-176 | W5 | Exact unicast IPv4 source/destination acceptance with verified UDP checksum |
| P0-177 | W5 | Reject foreign, multicast, loopback or reserved peers and NIC teardown races |
| P0-178 | W6 | Reject distinct ELF PT_LOAD byte regions that share a page-table entry |
| P0-179 | W6 | Enforce bounded aligned user stack with an unmapped guard page |
| P0-180 | W6 | Bind ELF page isolation and pinned image digest to one-time PID1 admission |

## Validation contract

The existing `.github/workflows/kernel-p0-six-workstations.yml` runs all previous P0 task families plus new `kernel_p0_batch9_w1..w6` regression tests and freestanding x86_64 `no_std` compilation on the exact PR head. Do not mark these tests as passed until GitHub verifies them. Independently check QEMU boot/persistence, M1 reliability, security, reviewed release evidence, and protected-branch rules.

## Explicit open acceptance

Physical APIC IPI/TLB shootdown, SMP runtime locks, real NIC IRQ/DMA/IOMMU, hardware TPM/UEFI provenance, actual ELF page mapping/ring3 boot, crash-consistent destructive power-loss recovery, and all inherited M1/R123 hardware/release gates remain open. The scheduler and IPC state transaction is in-memory only; its production caller must hold the authenticated topology and ordered kernel locks. Expected storage SHA-256 values must come from a trusted, signed external source. PR stays draft; no merge without all evidence.
