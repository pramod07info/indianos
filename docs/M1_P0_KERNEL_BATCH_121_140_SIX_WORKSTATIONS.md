# IndianOS Kernel P0 batch 7 — P0-121 through P0-140

Continuation of draft PR #786 from parent commit \`ba4d577d4a781c42c8f2ebaf78c3662759de130f\`. These are **source-level implementations and regression obligations**, not inherited hardware or release-gate acceptance. W1–W6 are six GitHub Actions validation workstations, **not six autonomous coding agents**.

| Task | Workstation | Implemented scope and required rejection |
| --- | --- | --- |
| P0-121 | W1 | Enforce one nonzero release generation for complete AP reclaim receipts |
| P0-122 | W1 | Reject acknowledged AP receipts from mixed epochs |
| P0-123 | W1 | Reject incomplete or duplicate AP topology entries |
| P0-124 | W1 | Reject pending APs and invalid/stale generation claims |
| P0-125 | W2 | Require same-epoch authenticated topology before process retirement |
| P0-126 | W2 | Reject mixed-epoch receipt proof without evicting victim |
| P0-127 | W2 | Roll back queue and notice publication if event ring is full |
| P0-128 | W2 | Protect PID1 and allow one correct generation-only retirement |
| P0-129 | W3 | Authenticate every queued relay prefix sender by exact PID/generation |
| P0-130 | W3 | Reject unknown sender without consuming the source FIFO |
| P0-131 | W3 | Preserve transaction if destination queue is full |
| P0-132 | W3 | Preserve ordered payloads with relay attribution on success |
| P0-133 | W4 | Two-field partition metadata read with paired verified observations |
| P0-134 | W4 | Keep both caller outputs untouched on late read fault or geometry failure |
| P0-135 | W4 | Permanently retire view if repeated metadata observations disagree |
| P0-136 | W5 | Validate UDP destination port and exact datagram length from verified IPv4 |
| P0-137 | W5 | Reject bad lengths and teardown races without delivering packet result |
| P0-138 | W6 | Hash actual pinned PID1 executable bytes against signed-manifest digest |
| P0-139 | W6 | Reject malformed entry/stack geometry before PID1 callback |
| P0-140 | W6 | Burn one-shot boot admission on failure and prevent replay |

## Exact-head CI and acceptance

The existing \`.github/workflows/kernel-p0-six-workstations.yml\` now executes \`kernel_p0_batch7_w1..w6\` regressions alongside all six earlier batches, and checks the freestanding \`x86_64-unknown-none\` \`no_std\` Rust build. These checks must **actually pass on this commit** before marking this batch verified.

**Still open and NOT credited to these tasks:** real APIC IPI/TLB shootdown, SMP runtime locking/integration, hardware NIC IRQ/DMA/IOMMU quiescence, authenticated TPM/UEFI/signed manifest provenance, trusted executable loader/page permissions, sudden power-loss recovery, final kernel QEMU boot/persistence and protected branch/independent reviewer acceptance. Keep draft; do not merge without all required evidence.
