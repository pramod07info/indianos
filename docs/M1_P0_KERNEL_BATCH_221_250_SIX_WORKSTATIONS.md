# IndianOS batch 12: P0-221–250 — exact-head source and test ledger

**Target:** draft [PR #786](https://github.com/pramod07info/indianos/pull/786), branch \`feat/p0-kernel-six-workstations-20-20261117\`, six CI verification workstations. **30 selected source-level obligations** divided into six areas. The workstations are CI jobs, **not** independently spawned developer agents. Integration in a single new commit on top of P0-220.

## Task-level implementation / regression evidence

| ID | Area | Concrete source change | Acceptance / negative test |
|---|---|---|---|
| 221 | W1 SMP | Define typed immutable user TLB shootdown request with exact epoch | Valid typed range |
| 222 | W1 SMP | Reject zero epoch, unaligned page, range overflow, >4096 pages | Invalid input sends zero IPI |
| 223 | W1 SMP | Deliver exact range alongside each AP's unique lifecycle receipt | Driver observes matching range |
| 224 | W1 SMP | Preserve AP receipt journal across multi-AP partial send | No fabricated acknowledgment on failed IPI |
| 225 | W1 SMP | Require exact epoch+receipt before AP local invalidation acknowledgment | Failed local operation cannot reclaim |
| 226 | W2 Process | Define integrated teardown including generation-scoped IPC | IPC included in retirement transaction |
| 227 | W2 Process | Pin integrated teardown to ingress revision and PID1/duplicate guards | Stale/duplicate batch leaves stores unchanged |
| 228 | W2 Process | Stage grant, live UDP endpoints, notices and IPC in separate copies | Late missing mailbox rejects all stores |
| 229 | W2 Process | Reap precise sender and receiver IPC queues on process exit | No old sender message delivered to successor |
| 230 | W2 Process | Publish all staged tables together only on full success | Socket removal and notice count observable together |
| 231 | W3 IPC | Accept authenticated multi-sender delivery batch | Distinct source identities preserved |
| 232 | W3 IPC | Check exact allowed live sender generations | Unknown/reincarnated sender denied |
| 233 | W3 IPC | Check aggregate byte quota with checked arithmetic | Overbudget batch yields no writes |
| 234 | W3 IPC | Apply repeated sends to cloned receiver queues before publication | Later full queue rolls back earlier send |
| 235 | W3 IPC | Preserve per-destination FIFO order on successful atomic publication | Two-sender FIFO ordering test |
| 236 | W4 Storage | Bind signed three-field metadata read to driver-owned media revision | Stale revision fails before I/O |
| 237 | W4 Storage | Read all three fields through disjoint signed partition offsets | Invalid/torn metadata rejected |
| 238 | W4 Storage | Re-observe full three-field set, including third field | Two complete triples verified |
| 239 | W4 Storage | Recheck media revision before, between and after observations | Mid-operation rebind poisons view |
| 240 | W4 Storage | Publish no output until both full passes agree; retire on fault | No partial output on injected I/O failure |
| 241 | W5 NIC | Accept zero-padded minimum Ethernet ARP reply | 60-byte real-format frame accepted |
| 242 | W5 NIC | Normalize exactly one explicit IEEE 802.1Q tag | VLAN 100 accepted, wrong tag denied |
| 243 | W5 NIC | Reject unexpected VLAN priority/DEI and nested tags | Foreign/trunk frame rejected |
| 244 | W5 NIC | Require exact peer/target MAC and IP with zero-only padding | Spoofing/padding denied before callback |
| 245 | W5 NIC | Preserve fenced live RX-epoch check through ARP parse callback | Racing quiescence discards parse |
| 246 | W6 PID1 | Introduce staged ELF load, guarded stack and atomic publish boundary | Trusted mapper interface |
| 247 | W6 PID1 | Pass exact verified ELF file slices, BSS size and permission bits | Mock validates bytes/zero-fill/flags |
| 248 | W6 PID1 | Fail image preflight before staging any page-table effect | Oversized BSS cannot invoke mapper |
| 249 | W6 PID1 | Invoke abort on failed load, stack or publish operation | Three injected failure stages |
| 250 | W6 PID1 | Consume authenticated admission exactly once even after failure | Replay rejected after success/failure |

## Exact-head CI proof requirements

Workflow: \`.github/workflows/kernel-p0-six-workstations.yml\` adds \`kernel_p0_batch12_w1\`…\`kernel_p0_batch12_w6\` and retains *all* prior P0 test filters plus x86_64 freestanding \`no_std\` builds. Exact SHA and six job conclusions must be recorded from GitHub, **not guessed**. The ledger is a trace of code obligations, not 30 completed high-level M1 hardware acceptance gates.

## Still open and not represented as implemented

The new APIs require an actual scheduler holding exclusive live locks and actual APIC/INVLPG hardware operations; they do not provide multicore TLB shootdown on physical devices. The transactional PID1 mapper is a **trait contract**, not a production x86 page-table mapper or ring-3 launch; real signed UEFI/TPM material, cross-reboot durability, physical NIC IRQ/DMA/IOMMU trust and power-cut tests remain open. Independent review, full kernel boot, persistent QEMU desktop, combined M1 reliability, security and protected merge are separate release gates.

**PR stays draft** until full exact-commit checks, independent review and branch protection evidence qualify it. Software-only tests must not close hardware tasks.
