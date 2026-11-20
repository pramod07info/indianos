# IndianOS P0 batch 20 — 40 production-integration obligations (481–520)

Existing draft PR: https://github.com/pramod07info/indianos/pull/786

**Scope:** Six actual kernel source files gain narrow production-path hardening and tests in this integration. This is NOT a claim that all 40 tasks are complete. The remaining physical multicore, DMA/IOMMU, TPM, crash/recovery and protected release acceptances stay OPEN.

## 40-item acceptance ledger

## W1 — PMM ownership and cross-core reclaim (481–487)

Dependencies: P0-441–447 / real physical frame lifecycle

| P0 ID | Task | Required acceptance | Source status |
|---|---|---|---|
| 481 | Guard real PMM run claims/releases against corrupt accounting counters | Production PMM claim_run/release_run preflight + negative Rust tests | PARTIAL — source + host tests |
| 482 | Attach owner-generation proof to every reusable frame | Exact owner identity must be checked at final release | OPEN |
| 483 | Reject stale same-physical-address frame retirement after PID reincarnation | Interleave freed and recycled frame owners on two CPUs | OPEN |
| 484 | Wait for every targeted AP TLB acknowledgment before reclaiming unmapped frames | Real APIC shootdown and stale-TLB fault injection | OPEN |
| 485 | Revoke DMA pin before freeing network or storage descriptor buffers | Device reset and IOMMU proof, not mock-only | OPEN |
| 486 | Prevent PMM reentrancy from IRQ and secondary CPUs without valid locking | Prove lock ordering and no accidental single-CPU alias | OPEN |
| 487 | Exercise real PMM out-of-memory, fragmentation and 4-vCPU stress | Guest serial evidence plus invariants and no double-free | OPEN |


## W2 — Timer session and scheduling correctness (488–494)

Dependencies: P0-448–454 / timer and runnable tasks

| P0 ID | Task | Required acceptance | Source status |
|---|---|---|---|
| 488 | Version the actual timer source session to reject same-source ABA timestamps | Epoch counters in kernel_monotonic_100hz and rearm/reset paths | PARTIAL — source + host tests |
| 489 | Check timer-source switchover under APIC/PIC reprogramming | No stale source receipt survives source switch | OPEN |
| 490 | Calibrate monotonic time across AP-local timer references | QEMU two-vCPU verified time source | OPEN |
| 491 | Detect exhaustion and non-monotonic IRQ clock observation | Hard fail on counter wrap and backward source | OPEN |
| 492 | Cancel sleep deadlines atomically across exit/restart | No wakeup for recycled PID generation | OPEN |
| 493 | Integrate timer clock into blocking wait and scheduler ready queue | Bounded timeout and progress under ring3 load | OPEN |
| 494 | Measure fairness, starvation and preemption latency under 4-vCPU QEMU | Logs show both CPU-bound ring3 tasks progress | OPEN |


## W3 — Ring-3 traps, syscalls and isolation (495–501)

Dependencies: P0-455–461 / protected userspace ABI

| P0 ID | Task | Required acceptance | Source status |
|---|---|---|---|
| 495 | Reject reserved x86-64 IRETQ flags in actual trap-frame admission | Production ring3_frame_valid and reserved-bit negative tests | PARTIAL — source + host tests |
| 496 | Audit SYSRET/IRET canonical return address and segment selectors | Malformed user return never reaches supervisor context | OPEN |
| 497 | Validate user buffers against actual installed user PTEs | Reject unmapped or supervisor-only page before copy | OPEN |
| 498 | Guarantee all-or-none scatter/gather user-copy commit | Failed later page leaves earlier destinations unchanged | OPEN |
| 499 | Authorize live syscall objects by process capability generation | Recycled PID cannot inherit stale handle authority | OPEN |
| 500 | Bound reentrancy, unknown syscall numbers and descriptor lengths | Fuzz all invalid syscall encodings and quotas | OPEN |
| 501 | Execute two adversarial ring3 processes in QEMU regression | Fault/exit/IPC cases and exact-head logs | OPEN |


## W4 — Durable block I/O and VFS recovery (502–508)

Dependencies: P0-462–468 / block and filesystem

| P0 ID | Task | Required acceptance | Source status |
|---|---|---|---|
| 502 | Pin media capacity across both stable metadata observations | Actual read_stable_sectors checks outer capacity and denies swapped device | PARTIAL — source + host tests |
| 503 | Bind metadata checksums to immutable selected partition identity | Reject same-content bytes read from another disk | OPEN |
| 504 | Connect acknowledged fsync to actual device sync/barrier semantics | No durable success on unsupported or failed cache flush | OPEN |
| 505 | Implement bounded write-ahead journal replay with torn-record detection | Crash at every write boundary leaves valid previous or committed state | OPEN |
| 506 | Make nested rename/unlink transactional across interruption | No directory cycles, lost acknowledged rename or leaked inode | OPEN |
| 507 | Lock namespace lookup and mount revision against concurrent replacement | Two-vCPU remount/open race rejects stale volume | OPEN |
| 508 | Prove Files GUI checksums across two cold boots and abrupt power interruption | QEMU disk artifact, checksums and serial logs | OPEN |


## W5 — NIC DMA, reset and socket ingress (509–514)

Dependencies: P0-469–474 / virtio device and network

| P0 ID | Task | Required acceptance | Source status |
|---|---|---|---|
| 509 | Recheck real PCI device identity after masking NIC bus mastering | Production VirtioNet close gate refuses changed/unreset hardware | PARTIAL — source + host tests |
| 510 | Mask and drain IRQ/MSI-X before freeing RX/TX queues | Real interrupt and completion evidence | OPEN |
| 511 | Tie every TX descriptor completion to exact device and ring generation | Reject post-reset completion for recycled buffer | OPEN |
| 512 | Fence physical NIC DMA/IOMMU domain before freeing pages | QEMU IOMMU plus physical hardware qualification | OPEN |
| 513 | Enforce live socket owner generation on packet TX/RX | Two ring3 processes cannot cross UDP endpoint ownership | OPEN |
| 514 | Run dual-NIC reset/link-flap traffic replay regression | PCAP and serial evidence of no unsafe reuse | OPEN |


## W6 — Boot handoff, trust and release qualification (515–520)

Dependencies: P0-475–480 / trusted normal boot

| P0 ID | Task | Required acceptance | Source status |
|---|---|---|---|
| 515 | Require a canonical Limine HHDM before kernel initialization proceeds | Production boot_validate refuses missing or malformed HHDM | PARTIAL — source + host tests |
| 516 | Reconcile every firmware memory-map reservation with PMM ownership | No frame from reserved or runtime spans can be claimed | OPEN |
| 517 | Validate signed kernel and recovery slot against immutable trust anchor | Tampered image never enters boot handoff | OPEN |
| 518 | Bind boot epoch and anti-rollback floor to TPM/UEFI provider | Physical authenticated measurement evidence | OPEN |
| 519 | Verify actual PID1 page-table bytes, permissions and guard before ring3 entry | Hardware PTE readback, one-shot authorized entry | OPEN |
| 520 | Run exact-head kernel boot, storage-reboot, NIC, ring3 and independent review gates | Non-skipped mandatory checks and protected merge evidence | OPEN |


## Source changes and testing boundaries

| Workstation | Production path changed | Test filter |
|---|---|---|
| W1 | `pmm.rs`: reserve/release refuses corrupted `used/total` counters | `kernel_p0_batch20_w1` |
| W2 | `timer.rs`: monotonic session epoch rejects stop/rearm ABA | `kernel_p0_batch20_w2` |
| W3 | `syscall.rs`: reserved RFLAGS blocked before ring-3 IRETQ | `kernel_p0_batch20_w3` |
| W4 | `block.rs`: double-read metadata tied to the same physical capacity | `kernel_p0_batch20_w4` |
| W5 | `virtio_net.rs`: reset and post-disable PCI ID verification before DMA release | `kernel_p0_batch20_w5` |
| W6 | `boot.rs`: Limine HHDM required at boot_validate, not just later | `kernel_p0_batch20_w6` |

All 6 dedicated CI verification jobs run nonempty Rust regression filters at the exact PR head, plus freestanding x86_64 `no_std` compilation. Original six-workstation and earlier batch suites remain intact.

**Important:** The six workstations are GitHub Actions test lanes, not six independently running coding agents. Host tests do not prove physical APIC/TLB, NIC DMA, TPM-backed boot, power-loss, or completed M1. The first task per workstation is marked PARTIAL pending guest/physical acceptance; the other 34 remain OPEN. Do not merge the draft PR before all mandatory checks and independent review pass.
