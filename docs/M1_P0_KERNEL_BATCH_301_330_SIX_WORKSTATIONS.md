# IndianOS P0 batch 15 — tasks 301–330

Target: existing draft [PR #786](https://github.com/pramod07info/indianos/pull/786). Source base: `293887598f4ac873fa1173040b00a3a941a28212`. Exactly **30 incremental source-level contracts** across six GitHub Actions verification workstations, not 30 hardware gates or 6 spawned developer agents.

## Task-to-implementation and evidence ledger

| P0 ID | CI workstation | Source-level scope | Regression evidence |
|---|---|---|---|
| 301 | W1 | Bound batched shootdowns at 64 spans and fixed output capacity | Nonempty and bounded plan tests |
| 302 | W1 | Canonicalize unaligned user byte spans to exact 4KiB pages | Cross-page calculation test |
| 303 | W1 | Reject overlapping virtual page coverage before IPI publication | Aliasing subpage regression |
| 304 | W1 | Reject zero epoch, zero length, overflow and user-limit crossing | Invalid epoch/range regressions |
| 305 | W1 | Preserve order and allow adjacent disjoint pages without coalescing | Adjacent-page regression |
| 306 | W2 | Stage supervisor lifecycle receipt delivery before dequeue | Exact two-receipt copy test |
| 307 | W2 | Do not modify output on late receipt mismatch | Late stale regression |
| 308 | W2 | Reject short delivery buffers without draining | Small output test |
| 309 | W2 | Reject duplicated or replayed sequence IDs | Duplicate and replay tests |
| 310 | W2 | Retain original FIFO on empty or malformed acknowledgement | Empty batch and FIFO tests |
| 311 | W3 | SHA-256 pin exact authenticated IPC FIFO head bytes | Happy path digest-pinned relay |
| 312 | W3 | Require exact sender generation at every source | Spoofed sender regression |
| 313 | W3 | Reject changed payload without consuming a source | Payload substitution regression |
| 314 | W3 | Preserve all FIFOs on late byte-quota or destination-full failure | Quota/capacity rollback tests |
| 315 | W3 | Reject duplicate source ownership before commit | Duplicate source regression |
| 316 | W4 | Bind two physically distinct mirror reads to a trusted manifest hash | Good mirrored manifest test |
| 317 | W4 | Do not publish identical but unauthenticated on-disk bytes | Digest mismatch test |
| 318 | W4 | Permanently retire media view on digest mismatch | Repeated read blocked test |
| 319 | W4 | Deny diverging mirror observations without partial output | Sector corruption regression |
| 320 | W4 | Reject invalid mirror geometry or changed media revision | Geometry/revision regression |
| 321 | W5 | Require a live per-device NIC RX admission for gateway ARP | Live permit test |
| 322 | W5 | Bind the ARP verdict to exact captured ring epoch | Wrong epoch regression |
| 323 | W5 | Reject foreign ring permit even if epoch numerically matches | Foreign ring regression |
| 324 | W5 | Check receive permit again after bounded ARP parsing | Quiesce revocation test |
| 325 | W5 | Deny invalid gateway ARP padding and permit after teardown | Corrupted padding/rebind tests |
| 326 | W6 | Require a mapper-provided nonzero staged PTE revision | Zero revision regression |
| 327 | W6 | Perform mandatory unpublished mapper audit before entry commit | Successful audit test |
| 328 | W6 | Compare PTE revision before and after audit | Drifting revision test |
| 329 | W6 | Abort on audit/revision failure and never publish entry | Abort and no-publication tests |
| 330 | W6 | Consume secure one-shot boot admission after mapper failure | Replay attempt denied test |

## Source and CI contract

W1: `smp_lifecycle_fence.rs`, `kernel_p0_batch15_w1`; W2: `process_events.rs`, `kernel_p0_batch15_w2`; W3: `ipc.rs`, `kernel_p0_batch15_w3`; W4: `block.rs`, `kernel_p0_batch15_w4`; W5: `network.rs`, `kernel_p0_batch15_w5`; W6: `r114_boot_epoch.rs`, `kernel_p0_batch15_w6`. Run all previous P0 regression suites in every W1–W6 job and exact-head freestanding `x86_64-unknown-none` `no_std` compilation. GitHub Actions results must be checked against the new commit SHA before marking *software validation* as passing.

**Implementation scope:** These helpers/guards are concrete host-testable source logic and interfaces. **Acceptance remains OPEN** for physical multicore APIC/IPI/INVLPG delivery, DMA/IOMMU/IRQ quiescence, production UEFI/TPM and ring-3 page tables, sudden disk power loss, real QEMU desktop persistence, independent security review and exact-head protected-branch merge. Existing public production entry points are not automatically rewired to call the new helpers. Keep the PR draft. No P0 hardware/release checkboxes are completed by these unit tests alone.
