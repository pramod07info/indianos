# IndianOS kernel P0 batch 4 — P0-61 through P0-80

Existing draft PR: https://github.com/pramod07info/indianos/pull/786
Base head: df24bfb4e4d75f87893705bbf380c144506e19e7
This additional batch does NOT close original M1 acceptance obligations.

| ID | Workstation | Required implementation and regression evidence |
| --- | --- | --- |
| P0-61 | W1 | Limit AP retirement verification to 1–64 receipts |
| P0-62 | W1 | Reject reuse of an identical AP lifecycle receipt |
| P0-63 | W1 | Reject repeated AP identity through distinct fences |
| P0-64 | W1 | Require nonzero exact-generation independent AP acknowledgements |
| P0-65 | W2 | Preflight all distinct AP proofs before process retirement |
| P0-66 | W2 | Refuse duplicate AP receipts without evicting a live task |
| P0-67 | W2 | Preserve process and events when exit notification queue is full |
| P0-68 | W2 | Publish exact-generation exit only after all AP acknowledgements |
| P0-69 | W3 | Stage IPC relay FIFO prefix and destination as one transaction |
| P0-70 | W3 | Authenticate exact relay process generation |
| P0-71 | W3 | Reject stale/full recipient without losing source message |
| P0-72 | W3 | Attribute forwarded messages to relay rather than spoofing sender |
| P0-73 | W4 | Twice observe complete bounded sector range in immutable media view |
| P0-74 | W4 | Keep caller output intact and poison view on late I/O failure |
| P0-75 | W4 | Reject torn second observation and out-of-range sector |
| P0-76 | W5 | Enforce packet size bounds before NIC parser callback |
| P0-77 | W5 | Revoke parsed RX result after device/quiesce/epoch race |
| P0-78 | W6 | Repeat trusted boot floor, slot and journal checks before PID1 |
| P0-79 | W6 | Consume one-shot launch receipt before the last-chance recheck |
| P0-80 | W6 | Deny late-sealed-floor rollback and replay of PID1 launch |

## Validation and known limitations

Six GitHub Actions CI matrix jobs (not six autonomous development agents) each
run their original, second, third and fourth P0 regression test filters
(kernel_p0_batch4_w1 through kernel_p0_batch4_w6), and freestanding
x86_64-unknown-none no_std compilation on the exact PR head.

The following remain OPEN regardless of software-host test success:
physical APIC IPI/TLB shootdown, hardware NIC IRQ/DMA/IOMMU,
production UEFI/TPM trust provider, production PID1 launcher wiring,
real interrupted-power durability, QEMU desktop gate, and exact-head
full kernel/S0 release and independent-review acceptance. Keep PR draft
until every required check is green and protected review is satisfied.
