# IndianOS kernel P0 batch 8 — P0-141 through P0-160

Continuation of existing draft PR #786, parent `44a410ce40e3b37255fc7dea2a777abdd06cd2e8`. This ledger tracks **20 distinct source implementation/test tasks**, not production hardware release acceptance. W1–W6 denote GitHub Actions validation jobs rather than separately launched coding agents.

| Task | Workstation | Implemented code and acceptance condition |
| --- | --- | --- |
| 141 | W1 | Retry a failed IPI send using its existing exact pending receipt |
| 142 | W1 | Never increment the generation or fabricate AP acknowledgement on resend |
| 143 | W1 | Reject foreign BSP/AP and forged receipt retries |
| 144 | W1 | Refuse retries of an already acknowledged/replaced publication |
| 145 | W2 | Require every authenticated AP to acknowledge the migration epoch |
| 146 | W2 | Stage multi-core migration before publishing real runqueue edits |
| 147 | W2 | Reject mixed/pending AP epochs without changing runqueue |
| 148 | W2 | Preserve all queues when affinity/capacity checks fail |
| 149 | W3 | Enforce a bounded cumulative payload byte budget per relay prefix |
| 150 | W3 | Reject over-budget transfer without consuming any FIFO messages |
| 151 | W3 | Preserve exact sender-generation and recipient ACL checks |
| 152 | W3 | Atomic rollback on target mailbox backpressure, preserve sender identity |
| 153 | W4 | Verify both partition metadata values against separately supplied SHA-256 |
| 154 | W4 | Never release a partially read/hash-failing pair to either caller buffer |
| 155 | W4 | Permanently retire read-only media view after authenticated digest mismatch |
| 156 | W5 | Validate strict IPv4 UDP pseudoheader/datagram checksum, including odd bytes |
| 157 | W5 | Reject unchecksummed/tampered UDP and stale NIC RX epoch result |
| 158 | W6 | Validate x86-64 ELF executable headers, offsets and non-overlapping PT_LOAD mappings |
| 159 | W6 | Reject W+X, executable stack, non-file-backed entry, bad ELF and dynamic interpreter |
| 160 | W6 | Bind ELF preflight and actual image SHA-256 to one-shot trusted PID1 admission |

## Verification contract

The existing `.github/workflows/kernel-p0-six-workstations.yml` adds `kernel_p0_batch8_w1..w6` tests and retains all preceding filter suites plus x86_64 freestanding no_std compile. Tests must pass against the **exact new PR head**; do not assert success based on previous commits. Recheck kernel boot, QEMU persistence, combined M1 reliability, S0 security, review and branch protection.

## Open acceptance boundaries

The software wrappers assume a privileged driver supplies real APIC delivery, local TLB invalidation and topology serialization. Actual IRQ/DMA/IOMMU quiescence, authenticated firmware/TPM and signed manifest origin, full ELF page mapping and ring-3 execution, journal crash/power-loss recovery and final M1/R123 release gates **remain open**. The metadata SHA-256 values must be pinned by trusted boot input; hashing disk bytes alone is not authentication. Keep PR draft and do not merge without full required evidence.
