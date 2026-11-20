# IndianOS kernel P0-201–220 — six workstation batch 11

Target: draft PR #786 on \`feat/p0-kernel-six-workstations-20-20261117\`.
Implementation comprises six existing kernel Rust modules and one exact-head CI matrix update. This is **one Git commit**, not twenty claimed independently approved production features. The six workstations are GitHub Actions **verification jobs**, not six concurrently executing coding agents.

## Source implementation and acceptance matrix

| ID | Workstation | Production change | Positive/negative test evidence |
|---|---|---|---|
| P0-201 | W1 SMP | Validate user TLB shootdown start alignment and minimum page | Unaligned and null ranges reject before IPI |
| P0-202 | W1 SMP | Cap 4096 pages and reject user canonical overflow | Size/overflow are rejected before IPI |
| P0-203 | W1 SMP | Publish per-AP receipts and preserve partial-send attempt journal | Second send fails while issued slots remain visible |
| P0-204 | W1 SMP | Prevent frame reclaim until exact AP local invalidation acknowledgment | Failed local proof does not acknowledge |
| P0-205 | W2 Process | Pin process-exit batch to current kernel NIC ingress revision | New process start invalidates stale snapshot |
| P0-206 | W2 Process | Reject PID1 and duplicate PID victims before any teardown | No notice on duplicate / PID1 |
| P0-207 | W2 Process | Validate live process generation before socket retirement | Active owner retained on failed batch |
| P0-208 | W2 Process | Atomically retire sockets and publish exit notice | Successful exact epoch releases port and enqueues once |
| P0-209 | W3 IPC | Stage one receive from every selected owner in copied router | Empty later inbox preserves earlier inbox |
| P0-210 | W3 IPC | Reject duplicate or reincarnated receiver/sender identities | Duplicate PID validation test |
| P0-211 | W3 IPC | Authenticate every live sender and check aggregate byte quota | Denied sender and quota preserve FIFO |
| P0-212 | W3 IPC | Commit all mailboxes only after successful user-copy callback | Copy fail retains both FIFO heads |
| P0-213 | W4 Storage | Preflight three fields for byte-level overlap | Zero disk reads on overlapping ranges |
| P0-214 | W4 Storage | Validate partition geometry and independently signed field positions | Bounded ranges and domain-bound SHA-256 |
| P0-215 | W4 Storage | Stage all outputs and re-observe earlier two fields after third | Fault/torn read poisons view; no partial output |
| P0-216 | W5 NIC | Verify ARP reply Ethernet/ARP header, peer MAC/IP, target MAC/IP | Spoofed header/identity callback never runs |
| P0-217 | W5 NIC | Tie parsed ARP reply to exact live NIC RX epoch | Quiesce racing parse rejects result |
| P0-218 | W6 PID1 | Build bounded ELF PT_LOAD file-copy, zero-fill and permissions descriptors | Valid ELF has correct file/BSS spans |
| P0-219 | W6 PID1 | Preflight descriptor output capacity; no partial plan on invalid ELF | Huge BSS and zero-capacity rollback |
| P0-220 | W6 PID1 | One-use Secure UEFI admission wraps pinned, checked ELF mapping plan | One successful launch, replay refused |

## Executable verification

Existing workflow: \`.github/workflows/kernel-p0-six-workstations.yml\`.
Additional Rust filters: \`kernel_p0_batch11_w1\` through \`kernel_p0_batch11_w6\`.
Each of the six jobs must enumerate and execute the matching regressions, run all ten earlier P0 filters, and compile x86_64 freestanding \`no_std\` at the **same exact head SHA**. CI output and run URLs must be checked after the commit is pushed; this document does not mark checks as passed.

## Still open — not covered by host software tests

- **Hardware:** actual xAPIC/x2APIC IPI delivery, per-core INVLPG/CR3 TLB invalidation, synchronized page-frame reclaim and final SMP races; NIC IRQ/DMA/IOMMU hardware, real ARP neighbor-cache integration.
- **Kernel path:** live SMP scheduler/IPC locking; physical power-loss and durable filesystem recovery, full ELF page-table allocation and ring-3 transfer; true signed UEFI/TPM authenticated providers.
- **Acceptance:** exact-PR-head kernel/QEMU boot, persistent desktop two-reboot tests, combined M1 reliability, S0 security, R123 inherited acceptance and protected branch independent review.

Do **not** merge draft PR #786, mark all M1 gates completed, or claim hardware acceptance merely because software regression jobs succeed.
