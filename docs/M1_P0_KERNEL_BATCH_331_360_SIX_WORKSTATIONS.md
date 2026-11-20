# IndianOS P0 batch 16 — tasks 331–360

Existing draft PR [#786](https://github.com/pramod07info/indianos/pull/786). Base source commit: `446f8b5583c99345939551d6c59a5d7a21e4c747`. Thirty **incremental source implementation obligations** (five per workstream), not hardware acceptance gate completion.

| ID | Workstation | Source change | Required evidence |
|---|---|---|---|
| 331 | W1 | Require exactly one AP lifecycle receipt for an ordered disjoint TLB plan | Disjoint flush test |
| 332 | W1 | Preflight per-span generation and page-range validity before a local callback | Epoch mismatch regression |
| 333 | W1 | Cap aggregate invalidation at 4096 pages across the complete plan | Over-budget regression |
| 334 | W1 | Reject overlapping or unsorted ranges before running invalidation | Order/overlap tests |
| 335 | W1 | Acknowledge AP only after every page flush succeeds; permit safe retry on partial failure | Failure, retry and replay tests |
| 336 | W2 | Transfer exact lifecycle receipts from kernel event source to supervisor queue | Two-ring forward test |
| 337 | W2 | Preserve FIFO order but issue fresh destination receipt sequence IDs | Receipt snapshot test |
| 338 | W2 | Roll back both queues when destination capacity rejects the final event | Late-full test |
| 339 | W2 | Restrict transferred process notices to trusted PID allowlist | Unauthorized PID test |
| 340 | W2 | Reject stale, duplicate and empty receipt batches without moving notifications | Replay and duplicate tests |
| 341 | W3 | Provide digest-pinned authenticated multi-inbox receive rather than relay | Batch receive test |
| 342 | W3 | Check sender process generation for every mailbox before publishing | Forged sender regression |
| 343 | W3 | Bound aggregate read bytes and destination capacity with atomic rollback | Quota and short-buffer tests |
| 344 | W3 | Reject duplicated source inboxes and stale receivers before commit | Duplicate/stale source tests |
| 345 | W3 | Leave delivery buffer and mailbox FIFOs unchanged on late hash mismatch | Digest mismatch regression |
| 346 | W4 | Read two separate manifest-authenticated redundant storage records atomically | Eight-read success test |
| 347 | W4 | Require four distinct bounded LBAs before any block I/O | Duplicate LBA regression |
| 348 | W4 | Keep both destination sectors intact if second record corrupts | Late corruption regression |
| 349 | W4 | Fence every read and final commit to one media revision | Changed revision regression |
| 350 | W4 | Return exact per-record receipts only after both SHA-256 checks succeed | Receipt digest tests |
| 351 | W5 | Wire a live NIC receive-fence into actual R028 QEMU ARP handshake | Real run_demo path updated |
| 352 | W5 | Acquire RX permit before actual virtio descriptor poll | Fenced poll test |
| 353 | W5 | Reject malformed padded ARP replies and bound polling | Corrupt padding/timeout tests |
| 354 | W5 | Revoke gateway success when software NIC quiesces during polling | Quiesce race test |
| 355 | W5 | Reject foreign device or quiesced ring without polling network device | Foreign NIC regression |
| 356 | W6 | Require mapper-provided digest of unpublished PID1 PTE and stack state | Trusted digest path |
| 357 | W6 | Reject non-authenticated zero expected PTE digest before mapping | Zero digest test |
| 358 | W6 | Verify mapping digest before and after mandatory final mapper audit | Stable digest test |
| 359 | W6 | Detect mapping-revision or PTE digest drift during final audit | Drift test |
| 360 | W6 | Abort and consume one-shot secure boot admission on changed mapping | Abort and replay test |

## Source-level coverage

- **W1** `rust/kernel_core/src/smp_lifecycle_fence.rs`: `kernel_p0_batch16_w1`
- **W2** `rust/kernel_core/src/process_events.rs`: `kernel_p0_batch16_w2`
- **W3** `rust/kernel_core/src/ipc.rs`: `kernel_p0_batch16_w3`
- **W4** `rust/kernel_core/src/block.rs`: `kernel_p0_batch16_w4`
- **W5** `rust/kernel_core/src/network.rs`: `kernel_p0_batch16_w5`
- **W6** `rust/kernel_core/src/r114_boot_epoch.rs`: `kernel_p0_batch16_w6`

One six-job GitHub Actions CI matrix must run each new filter plus all 15 previous batch filters and the freestanding `x86_64-unknown-none` `no_std` Rust build. Its results MUST be checked on the resulting exact commit SHA before any software success claim. These are six **test jobs**, not six concurrently launched implementation agents.

**Release/hardware gates still OPEN:** actual APIC IPIs and INVLPG/TLB physical execution and ordering; production multicore synchronization; NIC IRQ/DMA/IOMMU driver quiescence; live authenticated UEFI/TPM provider; production ring-3 mapper; sudden power-cut recovery; exact-head full QEMU persistent desktop and reliability; independent security review and protected merge. Some new software helpers require further integration before qualifying these gates. Leave PR draft.
