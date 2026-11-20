# IndianOS kernel P0 batch 5 — P0-81 through P0-100

Existing draft PR: https://github.com/pramod07info/indianos/pull/786
Parent PR head: `a1339308ad94261baa86ada66cef38527213786c`
Six W1–W6 GitHub Actions jobs validate distinct module families; they are **not six independent coding agents**.

| ID | CI workstation | Implemented kernel-code obligation |
| --- | --- | --- |
| P0-81 | W1 | Bind exact reclaim receipt batch to a single BSP ownership domain |
| P0-82 | W1 | Reject multi-BSP aggregation even if every AP independently acknowledged |
| P0-83 | W1 | Preserve successful same-BSP, distinct-AP barrier |
| P0-84 | W1 | Propagate invalid BSP domain as a fail-closed fence error |
| P0-85 | W2 | Reject two incarnations of one PID in a single retirement transaction |
| P0-86 | W2 | Preserve both runnable generations when a mixed-incarnation batch fails |
| P0-87 | W2 | Preserve exit-notification queue on invalid retirement |
| P0-88 | W2 | Propagate foreign-BSP rejection to process retirement |
| P0-89 | W3 | Atomically fan out relay FIFO messages to distinct exact-generation recipients |
| P0-90 | W3 | Roll back full/stale recipient failures without consuming source mailbox |
| P0-91 | W3 | Reject insufficient relay prefix and same-PID duplicate destinations |
| P0-92 | W3 | Attribute published traffic to authenticated relay, retaining payload FIFO |
| P0-93 | W4 | Read unaligned metadata fields across verified sector boundaries |
| P0-94 | W4 | Preserve caller destination after late I/O failure and poison the view |
| P0-95 | W4 | Reject overflow, invalid offset and spans beyond two sectors before I/O |
| P0-96 | W5 | Decode only bounded Ethernet IPv4 frames, with one optional VLAN tag |
| P0-97 | W5 | Reject malformed/truncated/non-IPv4 frames and RX epoch teardown races |
| P0-98 | W6 | Require trusted firmware Secure Boot before consuming PID1 launch receipt |
| P0-99 | W6 | Revalidate Secure Boot after admission and again at final handoff |
| P0-100 | W6 | Deny callback on late firmware downgrade and prevent receipt replay |

## Code and acceptance scope

- W1: `smp_lifecycle_fence.rs`, W2: `process_events.rs`, W3: `ipc.rs`, W4: `block.rs`, W5: `nic_rx_fence.rs`, W6: `r114_boot_epoch.rs`.
- Six new `kernel_p0_batch5_w1..w6` targeted Rust regression filters are mandatory in the exact-head CI matrix, alongside all four previous filter families and freestanding `no_std` compilation.
- Existing P0-01–80 changes and M1 release acceptance are preserved. This batch adds software guards and real code paths; it **does not** deliver physical APIC IPI/TLB invalidation, physical NIC IRQ/DMA/IOMMU ownership, a production TPM/UEFI implementation, live PID1 wiring, real sudden-power-loss durability or completed 13-gate M1 acceptance.
- CI results must be checked for the **new** commit. Leave the PR draft, do not mark inherited hardware release obligations complete, and do not merge pending exact-head verification and independent review.
