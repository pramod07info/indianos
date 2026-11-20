# IndianOS kernel P0 batch 10 — P0-181 through P0-200

Continuation of draft PR #786 based on previous head `02a47f4f3afe31b7b9e362e5e408a8b5bb83253a`.
Twenty source-level safeguard obligations across six GitHub Actions **CI verification workstations** (not six independent coding agents). None constitutes physical or M1/R123 release acceptance by itself.

| ID | Workstation | Kernel implementation | Acceptance evidence |
|---|---|---|---|
| P0-181 | W1 SMP/IPI | Preflight full authenticated AP retry roster | Invalid roster sends no IPI |
| P0-182 | W1 SMP/IPI | Bind pending receipts to exact BSP/AP and teardown epoch | Wrong AP/epoch rejected |
| P0-183 | W1 SMP/IPI | Journal every attempted hardware send even on failure | Partial-send attempt receipt |
| P0-184 | W1 SMP/IPI | Re-send only exact pending receipts and skip acknowledged APs | One retry then zero after ack |
| P0-185 | W2 Scheduler | Require exactly one runnable incarnation for each victim | Phantom victim rejected |
| P0-186 | W2 Scheduler | Reject recycled PID with wrong live generation | No rollback-visible mutation |
| P0-187 | W2 Scheduler | Protect PID1 and reject duplicate victim PIDs | Duplicate/PID1 regression |
| P0-188 | W2 Scheduler | Bind checked retirement to atomic scheduler/event/IPC staging | Three-store success / no partial exit |
| P0-189 | W3 IPC | Authenticate every exact sender incarnation against live router | Retired sender rejected |
| P0-190 | W3 IPC | Bind FIFO prefix to ordered sender identity list | Wrong ordering and short prefix rejected |
| P0-191 | W3 IPC | Check aggregate receive byte limit before user copy | Overquota FIFO unchanged |
| P0-192 | W3 IPC | Dequeue only after successful user-copy callback | Copy failure retains all messages |
| P0-193 | W4 Storage | Reject overlapping authenticated fields across sector boundaries | Cross-sector alias rejected before I/O |
| P0-194 | W4 Storage | Preflight partition geometry and field offsets | Invalid locations preserve outputs |
| P0-195 | W4 Storage | Use two-pass signed field verification; permanently retire torn media | Positive and torn-read tests |
| P0-196 | W5 Networking | Bind IPv4 unicast UDP flow to exact source and destination ports | Wrong source port rejected |
| P0-197 | W5 Networking | Enforce unicast MACs, nonzero TTL and NIC epoch freshness | Spoofed MAC/expired TTL/race rejected |
| P0-198 | W6 Boot/ELF | Cap total PID1 PT_LOAD/BSS/stack mapping pages | Huge BSS rejected |
| P0-199 | W6 Boot/ELF | Preserve W^X, shared-PTE isolation and unmapped stack guard | Prior page-isolation validator required |
| P0-200 | W6 Boot/ELF | Consume trusted launch admission even for rejected page plan | Failed plan cannot replay epoch |

## Verification contract

`.github/workflows/kernel-p0-six-workstations.yml` now runs all preceding P0 suites plus `kernel_p0_batch10_w1..w6` and the freestanding x86_64 `no_std` build at exact PR HEAD. These tasks are **code and regression coverage only**; do not mark as passed until the exact-head Actions jobs report success. Preserve full kernel/QEMU/persistence/security CI, independent review and protected merge requirements.

## Explicit remaining P0 acceptance

Real APIC IPI/TLB shootdown and synchronized SMP ownership; live scheduler/IPC kernel lock integration; NIC IRQ/DMA/IOMMU driver trust; hardware TPM/UEFI signed measurement; live ELF page tables and ring-3 PID1 startup; destructive-power-loss durability; QEMU desktop persistence and full M1/R123 release evidence remain OPEN. Keep PR draft and do not merge prematurely.
