# IndianOS R121 — five workstreams, 70 new tasks + 30 inherited obligations

Base main: `7e0b9b51e8c629f1961da6325129129422241a35`. R120 merged in PR #756. This checklist is **not** a claim that 70 or 100 tasks passed. It scopes 70 new acceptance items, plus 30 inherited release/blocker obligations from R120/R118/R119.

**Source changes in this one integration commit:** zero-receipt and concurrent-BSP publication exclusion, stable non-reusable NIC fence lifetime identity (including same-address driver replacement), selected mount-revision and PID-recycle regressions, and fail-closed PR draft/base-ref/base-SHA evidence with Rust/Python negative regressions. 29/70 new granular items are checked **at source scope**, pending exact-head CI after this amended commit. Their rows are intentionally granular acceptance checks, not separately delivered features. All 30 inherited physical/release obligations remain unchecked pending independent evidence.

### Workstream 1 — SMP lifecycle and scheduler fencing
- [x] R121-001 — Deny all-zero unpublished lifecycle receipts (source and focused regression authored; CI pending)
- [x] R121-002 — Deny zero-sequence acknowledgement forgery (source and focused regression authored; CI pending)
- [x] R121-003 — Deny zero-generation acknowledgement forgery (source and focused regression authored; CI pending)
- [x] R121-004 — Regress first legitimate unacknowledged receipt (source and focused regression authored; CI pending)
- [x] R121-005 — Regress first AP acknowledgement transition (source and focused regression authored; CI pending)
- [x] R121-006 — Verify one outstanding publication limit (source-level regression/guard; exact-head CI required)
- [x] R121-007 — Verify strictly increasing lifecycle generations (source-level regression/guard; exact-head CI required)
- [x] R121-008 — Verify BSP-only publication binding (source-level regression/guard; exact-head CI required)
- [x] R121-009 — Verify AP-only acknowledgement binding (source-level regression/guard; exact-head CI required)
- [x] R121-010 — Test two-vCPU release/acquire ordering in QEMU (source/guest regression authored, exact final-head CI required)
- [ ] R121-011 — Integrate IPI transport and completion fencing
- [ ] R121-012 — Guard actual SMP scheduler teardown lock order
- [ ] R121-013 — Add cross-core TLB shootdown acknowledgement
- [ ] R121-014 — Qualify physical multicore retirement stress

### Workstream 2 — NIC ingress and network epoch fencing
- [x] R121-015 — Bind quiescence receipt to exact fence instance (source and focused regression authored; CI pending)
- [x] R121-016 — Reject same-device foreign-instance receipt (source and focused regression authored; CI pending)
- [x] R121-017 — Reject foreign drained query without side effects (source and focused regression authored; CI pending)
- [x] R121-018 — Reject foreign finish-rebind without side effects (source and focused regression authored; CI pending)
- [x] R121-019 — Confirm valid same-instance completion after rejection (source and focused regression authored; CI pending)
- [x] R121-020 — Exercise in-flight RX permit drain ordering (source-level regression/guard; exact-head CI required)
- [ ] R121-021 — Enforce IRQ-time owner generation in production ISR
- [ ] R121-022 — Capture NIC revision before DMA ring fetch
- [ ] R121-023 — Revalidate NIC revision before UDP dispatch
- [x] R121-024 — Block stale RX after owner PID recycle (source/guest regression authored, exact final-head CI required)
- [ ] R121-025 — Stop hardware DMA before endpoint retirement
- [ ] R121-026 — Qualify MSI-X interrupt masking and completion drain
- [ ] R121-027 — Test multiple physical NIC instances with IOMMU
- [ ] R121-028 — Run independent cross-vCPU DMA/IRQ race harness

### Workstream 3 — Filesystem, persistence and boot trust
- [x] R121-029 — Audit canonical mount revision acquisition (source-level regression/guard; exact-head CI required)
- [ ] R121-030 — Detect path lookup during simultaneous mount replacement
- [x] R121-031 — Reject stale volume-token alias following remount (source-level regression/guard; exact-head CI required)
- [x] R121-032 — Exercise 64-cycle PID owner filesystem retire (source-level regression/guard; exact-head CI required)
- [ ] R121-033 — Prove lock-order deadlock freedom across VFS and UDP
- [ ] R121-034 — Test selected-volume stale token fuzz in QEMU
- [ ] R121-035 — Verify graphical Files retained after two cold boots
- [ ] R121-036 — Qualify physical SMP remount/lookup concurrency
- [ ] R121-037 — Integrate monotonic timestamp for storage flush
- [ ] R121-038 — Exercise injected torn boot journal writes
- [ ] R121-039 — Wire trusted boot measurement to immutable firmware root
- [ ] R121-040 — Verify boot epoch persistent observer at PID1 startup
- [ ] R121-041 — Qualify physical unsafe power interruption and recovery
- [ ] R121-042 — Qualify physical boot-sector restoration

### Workstream 4 — CI, code review and immutable evidence
- [x] R121-043 — Reject draft pull request in release verifier (source and focused regression authored; CI pending)
- [x] R121-044 — Reject non-main PR target before CI scan (source and focused regression authored; CI pending)
- [x] R121-045 — Revalidate base branch on final PR-head fetch (source and focused regression authored; CI pending)
- [x] R121-046 — Revalidate draft status on final PR-head fetch (source and focused regression authored; CI pending)
- [x] R121-047 — Add draft PR negative acceptance regression (source and focused regression authored; CI pending)
- [x] R121-048 — Add wrong-base negative acceptance regression (source and focused regression authored; CI pending)
- [x] R121-049 — Add mid-evidence base-change regression (source and focused regression authored; CI pending)
- [x] R121-050 — Add mid-evidence draft-transition regression (source and focused regression authored; CI pending)
- [ ] R121-051 — Require exact-head host Rust regression
- [ ] R121-052 — Require optimized freestanding no_std compilation
- [ ] R121-053 — Require exact-head QEMU boot, disk and network matrix
- [ ] R121-054 — Require human kernel and security reviewers
- [ ] R121-055 — Verify strict main branch protection and required checks
- [ ] R121-056 — Publish immutable post-merge main SHA evidence

### Workstream 5 — Integrated runtime and release qualification
- [ ] R121-057 — Recheck process-retirement live scheduler call chain
- [ ] R121-058 — Confirm two live ring3 process UDP owner isolation
- [ ] R121-059 — Verify forged foreign UDP close is denied
- [ ] R121-060 — Ensure normal exit and fault emit distinct generation receipts
- [x] R121-061 — Recheck PID generation recycling with delayed packets (source/guest regression authored, exact final-head CI required)
- [ ] R121-062 — Verify real polled virtio QEMU DNS and UDP failover
- [ ] R121-063 — Integrate IRQ-driven network ingress fence with sockets
- [ ] R121-064 — Run exact-head guest graphical Files and UDP acceptance
- [ ] R121-065 — Validate kernel ELF and ISO serial boot
- [ ] R121-066 — Require all workflow checks on identical source SHA
- [ ] R121-067 — Demonstrate non-reentrant BSP critical section under IRQ masking
- [ ] R121-068 — Qualify IPI scheduler exit delivery on actual multicore
- [ ] R121-069 — Qualify real NIC interrupt and DMA isolation
- [ ] R121-070 — Confirm release decision after independent approval

### Inherited 30 obligations — still not independently accepted

- [ ] R121-I01 — R120-013 — cross-core lock-free access protection
- [ ] R121-I02 — R120-026 — real interrupt ingress generation ownership
- [ ] R121-I03 — R120-028 — physical multi-NIC IRQ and IOMMU qualification
- [ ] R121-I04 — R120-042 — cross-core IPI lifecycle ordering
- [ ] R121-I05 — R120-070 — independent review and protected merge evidence
- [ ] R121-I06 — R118-001 — cross-BSP mount revision synchronization
- [ ] R121-I07 — R118-003 — concurrent mount replacement stale revision denial
- [ ] R121-I08 — R118-009 — deterministic deadlock-free file-owner retirement
- [ ] R121-I09 — R118-013 — selected-volume token fuzz in QEMU
- [ ] R121-I10 — R118-014 — physical SMP remount/lookup concurrency
- [ ] R121-I11 — R118-027 — two-BSP scheduler revocation stress
- [ ] R121-I12 — R118-028 — actual multicore teardown and IPI safety
- [ ] R121-I13 — R118-040 — foreign port close denial guest acceptance
- [ ] R121-I14 — R118-041 — calibrated kernel monotonic source
- [ ] R121-I15 — R118-042 — physical NIC ingress IRQ qualification
- [ ] R121-I16 — R118-054 — immutable production bootloader trust anchor
- [ ] R121-I17 — R118-055 — physical destructive power-loss recovery
- [ ] R121-I18 — R118-056 — hardware boot-sector restoration
- [ ] R121-I19 — R118-062 — exact-head optimized freestanding build
- [ ] R121-I20 — R118-063 — exact-head host Rust test matrix
- [ ] R121-I21 — R118-064 — two-boot persistent graphical Files QEMU
- [ ] R121-I22 — R118-065 — real ring3 two-process UDP QEMU
- [ ] R121-I23 — R118-066 — QEMU boot and network matrix
- [ ] R121-I24 — R118-067 — independent kernel/security review
- [ ] R121-I25 — R118-068 — exact integration SHA workflow check
- [ ] R121-I26 — R118-070 — protected post-merge main evidence
- [ ] R121-I27 — R119-027 — group retirement through live network table
- [ ] R121-I28 — R119-041 — scheduler fault and normal-exit integration
- [ ] R121-I29 — R119-056 — cross-core IRQ scheduler delivery
- [ ] R121-I30 — R119-069 — all mandatory checks green on final source SHA

## Release acceptance and limitations

- No actual five autonomous agents were spawned; the five workstreams are an integration review structure.
- R121 CI repair (2026-11-03): the previous kernel ELF/ISO job built host Rust successfully but failed posting `indianos/rust-tools` with HTTP 503. Later required QEMU steps were skipped. This was a transient GitHub API publishing failure, not evidence of a kernel compile error. A shared bounded 429/5xx/network retry helper now guards all twenty exact-SHA workflow status POSTs; exhausted or non-retryable errors still fail CI. Unit tests cover exact request identity and fail-closed outcomes.
- R121 AP replay hardening: the actual two-QEMU-vCPU Limine AP test now rejects a repeated acknowledgement for each of 64 published lifecycle receipts. The sequence is accepted only once through atomic compare/exchange; the QEMU marker is emitted only when the guest proof returns success.
- R121 NIC replay hardening: live coordinator, actual UDP queue, BSP admission gate and NIC RX permit are exercised in focused host tests across PID restart, port reuse and quiescing. Physical IRQ-driven NICs and IOMMU remain unqualified.
- Added source regressions for 64 alternating mount-revision/owner-recycle cycles; they are NOT an actual remount race on two CPUs or a physical disk.
- Address re-use in the NIC queue controller is covered by opaque, non-repeating generation IDs; true NIC DMA/multi-NIC hardware validation is still pending.
- The exact-head GitHub CI must be rerun because this replaces the original R121 commit SHA while keeping a one-commit PR.
- Do not merge based only on a prior green SHA; require exact-head host Rust, optimized no_std, kernel ELF/ISO, graphical persistent Files, two ring3 UDP owners, real QEMU NIC and security workflows.
- The source-level tests above do not establish real IRQ-driven ingress, physical SMP, IOMMU, boot firmware trust, or abrupt power-failure recovery.
- A passing PR workflow is not independent human approval. Protected merge requires the project's explicitly configured review and branch-protection gates.
- Continue the complete R120 inherited ledger at `docs/M1_R120_100_TASK_KERNEL_RETIREMENT.md`; the 30 above are prioritized carry-forwards, **not** the only remaining work.
