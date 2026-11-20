# IndianOS P0 batch 13: 251–280

**Target:** existing draft [PR #786](https://github.com/pramod07info/indianos/pull/786) on `feat/p0-kernel-six-workstations-20-20261117`. Exactly thirty distinct, source-level P0 IDs across six CI-verification workstations. This is one consolidated source commit, not six independently spawned coding agents.

## Implementation and test ledger

| ID | Workstation | Change | Required software acceptance |
|---|---|---|---|
| P0-251 | W1 SMP/TLB | Add trusted local per-page invalidation callback interface | Exact page addresses observed |
| P0-252 | W1 SMP/TLB | Enumerate all bounded 4 KiB pages of an exact validated range | One callback per requested page |
| P0-253 | W1 SMP/TLB | Require exact AP, sequence and publication epoch before flush | Wrong AP/epoch calls no driver |
| P0-254 | W1 SMP/TLB | Reject incomplete local page flush without AP acknowledgment | Partial failure leaves pending receipt |
| P0-255 | W1 SMP/TLB | Prevent duplicate flush/acknowledgment on replay | Replay performs no callback |
| P0-256 | W2 Process | Add exact-generation faulted-process four-store retirement | Fault notice and UDP revoke |
| P0-257 | W2 Process | Pin fault teardown to ingress revision | Stale revision rejected without mutation |
| P0-258 | W2 Process | Reject PID1, duplicate/reincarnated victims | Preflight rejects without side effects |
| P0-259 | W2 Process | Stage capabilities, UDP endpoints, notices and IPC copies | Late missing mailbox rolls back all |
| P0-260 | W2 Process | Atomically publish faulted process retirement | Survivor messages and ownership remain |
| P0-261 | W3 IPC | Add exact generation-checked relay across distinct source mailboxes | Two sources relay successfully |
| P0-262 | W3 IPC | Require live trusted relay registration and allowlisted origins | Forged origin rejected |
| P0-263 | W3 IPC | Enforce checked aggregate relay byte quota | Over-budget retains source queues |
| P0-264 | W3 IPC | Stage source dequeue and target sends in one copied router | Empty later source/full target rollback |
| P0-265 | W3 IPC | Preserve destination FIFO and disclose relay as visible sender | Successful target order and provenance |
| P0-266 | W4 Storage | Retire read-only view on stale pre-I/O media revision | Zero reads and permanent retirement |
| P0-267 | W4 Storage | Bind observed triple to partition geometry and exact field offsets | Domain fingerprint differs by position |
| P0-268 | W4 Storage | Include trusted externally supplied three digests in fingerprint | Digest-specific SHA-256 transcript |
| P0-269 | W4 Storage | Include media generation in bounded diagnostic receipt | Different media revision differs |
| P0-270 | W4 Storage | Return receipt only after double triple-read validation | No receipt/output on invalid geometry or changed media |
| P0-271 | W5 Network | Harden **live** virtio-net polled gateway ARP parser | Valid unpadded and padded reply |
| P0-272 | W5 Network | Reject truncated partial padding or oversized packet | Invalid 43–59/65+ lengths denied |
| P0-273 | W5 Network | Deny nonzero trailing payload after ARP body | Padding injection test |
| P0-274 | W5 Network | Maintain exact trusted MAC/IP/opcode peer identity | Spoofed source/target/opcode denied |
| P0-275 | W5 Network | Fail closed on unsupported VLAN/trunk EtherType | VLAN frame cannot resolve untagged peer |
| P0-276 | W6 PID1 | Add final authenticated secure-firmware chain recheck after staging | Downgrade stops publication |
| P0-277 | W6 PID1 | Re-read sealed rollback epoch/journal/slot policy before publication | Raised hardware floor blocks launch |
| P0-278 | W6 PID1 | Re-read selected bootloader slot and pinned version | Unexpected slot/version blocks launch |
| P0-279 | W6 PID1 | Re-read authenticated image/boot measurement | Measurement swap aborts staged mapper |
| P0-280 | W6 PID1 | Abort staged loader when late trust check fails, preserving one-shot epoch | Failure rolls back; restored trust cannot replay |

## Verification gates

The existing workflow `.github/workflows/kernel-p0-six-workstations.yml` must execute `kernel_p0_batch13_w1` through `kernel_p0_batch13_w6`, all prior P0 test families, exact PR-head SHA validation, and freestanding x86_64 `no_std` compilation for each workstation. A separate outcome check is required for QEMU kernel boot, persistent VFS/desktop, combined reliability, R114, R126, S0 and release evidence. **Do not claim any CI success before the exact-head check is green.**

## Inherited requirements that remain OPEN

Physical xAPIC/x2APIC send and INVLPG execution, frame-reclaim ordering across real cores, hardware NIC DMA/IOMMU and IRQ ownership, production UEFI/TPM signed provider, real ring-3 ELF page-table mapper, disk power-cut recovery, physical networking and protected merge/independent acceptance. A callback contract or a host Rust test does not close these hardware or end-to-end tasks. Keep PR draft until all required evidence is obtained.
