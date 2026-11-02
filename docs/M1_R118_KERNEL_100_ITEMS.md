# IndianOS R118 — 100-item kernel-first acceptance ledger

Base: `90656c3261bb6c4007f39e2a7d4a5403dc561f74` (merged R117 PR #749).

**Evidence rules:** `[x]` below means a source-level implementation or regression was added in this R118 commit, **not** a green exact-head CI verdict, physical hardware qualification, or a completed independent kernel subsystem. The R117 78/100 baseline is separate. No independent agent workstations were launched; these are five engineering workstreams. A failed or pending CI check blocks merge.

**This batch does not claim that the requested 70 new tasks are all complete.** Items without actual implementation or matching verification remain unchecked. Avoid interpreting this ledger as a kernel completion percentage.


## W1 Filesystem and mount correctness

- [ ] R118-001 — Audit cross-BSP mount revision synchronization
- [x] R118-002 — Define documented lock order for namespace receipt admission
- [ ] R118-003 — Reject stale mount revision after concurrent replace
- [ ] R118-004 — Preflight partition identity before first disk read
- [x] R118-005 — Guard selected-volume receipt after mount probe
- [x] R118-006 — Fail closed when volume geometry changes
- [x] R118-007 — Reject cached handle after recovery revision
- [x] R118-008 — Preserve filesystem owner grants on failed process batch
- [ ] R118-009 — Prove deterministic deadlock-free file-owner retirement
- [x] R118-010 — Propagate write uncertainty to quarantine
- [x] R118-011 — Keep revision monotonic across full remount
- [x] R118-012 — Verify alias invalidation after PID reuse
- [ ] R118-013 — Exercise selected-volume stale-token fuzz in QEMU
- [ ] R118-014 — Qualify true SMP concurrent remount/lookup

## W2 Scheduler and capability retirement

- [x] R118-015 — Define file/network/capability teardown lock order
- [ ] R118-016 — Audit scheduler exit from normal return
- [ ] R118-017 — Audit scheduler exit from fault path
- [x] R118-018 — Reject stale PID generation during revocation
- [ ] R118-019 — Revoke delegated directory lease at owner exit
- [x] R118-020 — Revoke exact owner UDP ports at exit
- [x] R118-021 — Preserve peer PID leases on owner fault
- [x] R118-022 — Close outstanding file handles atomically
- [x] R118-023 — Reject replayed lifecycle exit acknowledgement
- [x] R118-024 — Verify event FIFO head after wraparound
- [ ] R118-025 — Check scheduler rollback cannot resurrect owner
- [ ] R118-026 — Stress three-process mixed resource teardown
- [ ] R118-027 — Run two-BSP scheduler revocation stress
- [ ] R118-028 — Qualify real multicore IPI and teardown safety

## W3 Network transmit and ingress hardening

- [x] R118-029 — Verify authenticated route policy before frame preparation
- [x] R118-030 — Bind initial route receipt to exact owner generation
- [x] R118-031 — Reject expired primary lease
- [x] R118-032 — Reject expired backup receipt
- [x] R118-033 — Recheck current receipt just before NIC dispatch
- [x] R118-034 — Sample second clock AFTER frame serialization
- [x] R118-035 — Reject backwards-moving clock at dispatch
- [x] R118-036 — Deny Ethernet send on post-serialization expiry
- [x] R118-037 — Preserve NIC send counter after denied dispatch
- [x] R118-038 — Accept stable clock on valid leased route
- [x] R118-039 — Recheck current owner policy on dispatch
- [ ] R118-040 — Prove foreign UDP port close denial in QEMU
- [ ] R118-041 — Integrate calibrated kernel monotonic source instead of test clock
- [ ] R118-042 — Qualify ingress IRQ under multiple physical NICs

## W4 Verified boot epoch and physical recovery

- [x] R118-043 — Use single verified measurement snapshot for guarded advance
- [x] R118-044 — Reject changing trust observer mid-transaction
- [x] R118-045 — Reject mismatched initial measurement without media write
- [x] R118-046 — Retain sealed minimum epoch check
- [x] R118-047 — Retain minimum signed slot version check
- [x] R118-048 — Retain selected-slot authorization
- [x] R118-049 — Reject journal generation rollback
- [x] R118-050 — Keep two-copy predecessor digest binding
- [x] R118-051 — Detect torn newest journal copy
- [x] R118-052 — Refuse divergent same-epoch journals
- [x] R118-053 — Retain readback and flush requirements
- [ ] R118-054 — Wire production bootloader trust anchor
- [ ] R118-055 — Qualify real unsafe physical power cut
- [ ] R118-056 — Verify hardware boot-sector restoration

## W5 Runtime, QEMU and release qualification

- [x] R118-057 — Preserve legacy network/boot Rust public APIs
- [x] R118-058 — Add regression for clock sampling after serialization
- [x] R118-059 — Add regression for stable/reversed NIC clock
- [x] R118-060 — Add regression for one-snapshot sealed commit
- [x] R118-061 — Add regression for mismatched boot snapshot
- [ ] R118-062 — Require freestanding optimized no_std build
- [ ] R118-063 — Require exact-head host Rust unit suite
- [ ] R118-064 — Require two-boot persistent graphical Files QEMU
- [ ] R118-065 — Require real two-process ring3 port-ownership QEMU
- [ ] R118-066 — Require full QEMU network and boot matrix
- [ ] R118-067 — Require independent kernel/security review
- [ ] R118-068 — Confirm CI exact integration SHA
- [x] R118-069 — Record scope distinction between QEMU and physical hardware
- [ ] R118-070 — Publish protected merge and post-merge main evidence

## Inherited from merged R117: 22 unresolved gates

- [ ] R118-071 — Inherited R117-071: Inherited R116-071 — Inherited R115-074 — Reject concurrent mount replacement during path lookup
- [ ] R118-072 — Inherited R117-073: Inherited R116-073 — Inherited R115-078 — Enforce port-generation checks at network ingress IRQ
- [ ] R118-073 — Inherited R117-076: Inherited R116-076 — Inherited R115-084 — Audit lock ordering of file/network lease teardown
- [ ] R118-074 — Inherited R117-077: Inherited R116-077 — Inherited R115-085 — Prove no scheduler teardown deadlock in SMP configuration
- [ ] R118-075 — Inherited R117-078: Inherited R116-078 — Inherited R115-086 — Use trusted boot observer in production PID1 boot path
- [ ] R118-076 — Inherited R117-079: Inherited R116-079 — Inherited R115-087 — Provision immutable boot measurement root in bootloader
- [ ] R118-077 — Inherited R117-080: Inherited R116-080 — Inherited R115-088 — Verify boot journal under actual unsafe power cut
- [ ] R118-078 — Inherited R117-081: Inherited R116-082 — Inherited R115-090 — Qualify physical boot-sector restoration on hardware
- [ ] R118-079 — Inherited R117-082: Inherited R116-083 — Inherited R115-092 — Read a kernel monotonic timestamp at transmit and retry
- [ ] R118-080 — Inherited R117-083: Inherited R116-085 — Inherited R115-095 — Qualify multiple physical NICs with IOMMU isolation
- [ ] R118-081 — Inherited R117-085: Inherited R116-087 — Inherited R115-097 — Pass all host Rust tests on the exact integration commit
- [ ] R118-082 — Inherited R117-087: Inherited R116-090 — Inherited R115-100 — Complete independent review and merge only with green required CI
- [ ] R118-083 — Inherited R117-090: Inherited R116-093 — Implement physical multicore scheduler lock ordering and IPI safety
- [ ] R118-084 — Inherited R117-091: Inherited R116-094 — Provide immutable production bootloader measurement root
- [ ] R118-085 — Inherited R117-092: Inherited R116-096 — Verify actual physical power-loss recovery and boot-sector repair
- [ ] R118-086 — Inherited R117-093: Inherited R116-097 — Qualify multiple physical NICs with IOMMU DMA isolation
- [ ] R118-087 — Inherited R117-094: Inherited R116-098 — Require exact commit host Rust/no_std, QEMU network, graphical VFS and boot workflows
- [ ] R118-088 — Inherited R117-095: Inherited R116-099 — Complete independent review and branch protection before merge
- [ ] R118-089 — Inherited R117-096: Inherited R116-100 — Publish verified post-merge main SHA and record release evidence
- [ ] R118-090 — Inherited R117-097: Integrate a hardware-calibrated kernel monotonic clock into live NIC dispatch
- [ ] R118-091 — Inherited R117-099: Complete a final exact-head persistent graphical Files reboot and release matrix
- [ ] R118-092 — Inherited R117-100: Publish independent review, protected merge SHA and hardware-vs-QEMU evidence

## Final 8 acceptance tasks (not waived)

- [ ] R118-93 — Exact-head Rust host suite passes
- [ ] R118-94 — Exact-head optimized no_std build passes
- [ ] R118-95 — QEMU persistent Files cold reboot passes
- [ ] R118-96 — Real ring3 two-process UDP QEMU acceptance passes
- [ ] R118-97 — QEMU boot-epoch failure matrix passes
- [ ] R118-98 — All required workflows green on same final SHA
- [ ] R118-99 — Independent code review and branch protection complete
- [ ] R118-100 — Publish verified merged-main SHA and physical-hardware evidence gaps

## Implemented in current source commit

- `network.rs`: move the last trusted time sample after validated IPv4 construction and immediately before `EthernetDevice::send`; recheck exact route owner and time-scoped receipt at that point. Existing public API preserved.
- `r114_boot_epoch.rs`: snapshot the platform-authenticated boot measurement once for the complete guarded read/advance operation, preventing a time-of-check/time-of-use identity change. Still a fixture in QEMU, **not** TPM/UEFI measured boot.
- Targeted Rust regression tests for post-serialization lease expiration, stable/backward clock, and changing/mismatched measurement source.

## Known constraints

- No physical SMP, APIC IRQ ownership, IOMMU DMA isolation, immutable bootloader root, or unsafe power cut is certified here.
- CI must run on the exact PR SHA before promoting any of the source-only items to verified acceptance; a new Markdown-only commit changes the SHA.
- Actual 5 independent autonomous workstations were not launched; one integrated commit is used.

## R118 continuation — additional implementation in the same PR

- `network.rs`: prevent unauthenticated QEMU Ethernet MAC redirects, IPv4 fragments, unvalidated trailing bytes and unsupported IPv4 header lengths before the actual NIC send. The policy/clock dispatch guard remains after serialization.
- `network.rs` + `user_runtime.rs`: exact-generation scheduler UDP cleanup now retires orphan registry grants when a previous close already removed the socket entry, without revoking a reused PID or live peer.
- `r114_boot_epoch.rs`: guarded boot advancement verifies the *same exact predecessor epoch record* again at the write boundary. A disk switching to a checksum-valid same-epoch but different journal-generation record between observations cannot authorize a write.
- New host tests exercise the real registry/socket functions and fail-closed IPv4 transmit, plus a custom block device which swaps its journal on the second observation.
- No new physical root of trust, physical SMP, IOMMU or genuine power-failure lab qualifications are claimed. CI needs to rerun on the new exact PR head.

## R118 continuation: one-time PID1 lifecycle receipts and selected-disk geometry

- `process_events.rs`: add monotonic per-enqueue `EventReceipt` IDs, exact-head one-time acknowledgement, zero-capacity and overflow fail-closed semantics, plus replay/wrap/fuzz-style unit tests. The legacy value-only acknowledgement remains for compatibility and is **not** replay-safe for duplicate message payloads.
- `user_runtime.rs`: privileged PID1 lifecycle consumer uses sequence-bearing receipts rather than comparing only `ProcessEvent` payloads. The queue remains single-BSP, not SMP-safe.
- `fs_handles.rs`: before selected-partition sector I/O, reject parent-device capacity changes against the kernel-issued receipt. Complete GPT/MBR/FS revalidation remains mandatory at mount time.
- This completion checks R118-006, R118-023 and R118-024 as bounded source behaviors, **not** physical SMP qualification, IRQ correctness, or release acceptance. Exact-head CI must pass before merger.

### R118 event compaction hardening
All four lifecycle cleanup methods now preserve the surviving event's unique sequence ID during FIFO compaction without allocating a new sequence ID. Cleanup cannot panic solely because the monotonic counter reached its limit. Dedicated wrap/exhaustion regression tests added; no SMP synchronization claim.

## R118 continuation — source implementation and validation expansion

The additions below cover 19 **specific** ledger items with bounded new implementation, regression tests, or explicit single-BSP engineering contracts. They do NOT certify 19 completed hardware subsystems.

**Filesystem — R118-002/005/008/011/012**
- Explicit resource admission/lock-order contract in `docs/M1_R118_RESOURCE_TEARDOWN_ORDER.md`, limited to exclusive single-BSP mutators.
- Fail closed on volume-revision exhaustion **before** any disk read.
- Publish the accepted namespace only after complete selected-volume receipt and health verification.
- Exercise failed file-owner batch preserving handles/aliases and successful retirement preventing recycled-PID handle reuse.

**Scheduler — R118-015/018/021/022**
- Explicit serialized teardown order: lifecycle event, network port registry, delegated grants, file handles, then fallible memory reclamation.
- Add exact-incarnation registered UDP batch retirement, preflighting every PID/generation against both socket and registry state.
- Add staged capability lease batch revocation, blocking partial cleanup of peers when an owner generation is stale.
- Exercise three live network owners, orphan grant cleanup, peer delivery after owner exit, and no resurrection of old PID handles.

**Network — R118-031/032/033**
- Test exact primary and backup DHCP lease expiry **after** frame serialization, clock reversal, and route-generation retirement before NIC dispatch. Denied operations produce no `EthernetDevice::send`.

**Verified boot — R118-046/047/048/050/051/052/053**
- Add hostile sealed-floor matrix: minimum epoch/generation/slot version, selected-slot authorization, uncommitted metadata/attempts, forged predecessor SHA, divergent same-epoch two-copy journals, torn newest-copy rollback floor, and failed durable flush.
- All recovery and power loss tests here are model-level. Actual abrupt physical power cut and immutable TPM/UEFI root remain **unverified**.

**Honest acceptance:** 40/100 source/test/document items checked, 60 items open. The 22 inherited R117 gates and the eight final release gates remain unchecked. New exact-head host Rust, freestanding build and QEMU workflows must rerun; prior green checks do not authorize merge. Five workstreams are logical, not five deployed autonomous coding agents.

## Six commits / cohort 1 — filesystem and scheduler admission
Cohort R118-001, R118-003, R118-004, R118-007, R118-009, R118-010, R118-013, R118-014, R118-016, R118-017. Implemented source: a failed write-time mount verification on an already selected disk quarantines cached handles; recovery revision exhaustion rejects without disk reads. Two regression tests. This does not complete the ten-item cohort; the physical SMP and QEMU gates remain open.

## Six commits / cohort 2 — scheduler and NIC authority
Cohort: R118-019, R118-025, R118-026, R118-027, R118-028, R118-040, R118-041, R118-042, R118-054, R118-055. Implemented: enforce registry/socket generation preflight in single-owner scheduler retirement; add host regression for forged UDP close, IRQ-equivalent ingress generation check, and stale process cleanup after PID reuse. This does **not** prove QEMU IRQ or physical multicore operation. Bootloader trust and unsafe physical power-cut remain OPEN.

## Six commits / cohort 3 — test and release evidence
Cohort: R118-056, R118-062, R118-063, R118-064, R118-065, R118-066, R118-067, R118-068, R118-069, R118-070. Added a standard-library Python exact-head read-only GitHub checker and hardware-vs-QEMU acceptance matrix. R118-069 is documentation-complete; other cohort items remain OPEN until evidence exists, including all hardware and protected-release gates.

## Six commits / cohort 4 — inherited R117 hardware readiness
Cohort: R118-071, R118-072, R118-073, R118-074, R118-075, R118-076, R118-077, R118-078, R118-079, R118-080. Added cooperative mount/namespace/reused-PID and UDP generation regressions plus physical qualification matrix. None of the ten inherited hardware and multi-core requirements is fully validated; all remain OPEN.

## Six commits / cohort 5 — exact-SHA and reviewer verification
Cohort: R118-081, R118-082, R118-083, R118-084, R118-085, R118-086, R118-087, R118-088, R118-089, R118-090. Hardened release verifier to reject outdated independent approvals and unresolved CHANGES_REQUESTED review; added seven Python stdlib tests for exact SHA, incomplete required CI, author self-review, stale approvals, blocking reviews and missing branch protection. Physical SMP, immutable firmware root, real power-loss, IOMMU and clock hardware gates remain OPEN. Tests are code-level until CI explicitly executes them.

## Six commits / cohort 6 — final release and CI acceptance
Cohort: R118-091, R118-092, R118-93, R118-94, R118-95, R118-96, R118-97, R118-98, R118-99, R118-100. Added read-only release evidence verifier unit tests to GitHub Actions, and a 10-gate hold matrix. **No final release gate is marked complete until exact final commit CI, QEMU acceptance, independent review, branch protection, verified main SHA, and hardware gap notes exist.**

Six-commit batch record (each commit message contains exactly 10 assigned task IDs):
1. `0acce6042807bedfa04918548c4afa8009956d01` — cohort 1/6
2. `8f8620428cdc531b11b139d26fe0c0570cf89c0f` — cohort 2/6
3. `c2b32bf52e1702ceb0bd043a9cd1318ca9cf5b12` — cohort 3/6
4. `9f6721e578305276d2b5ff93dfafee8bde81b638` — cohort 4/6
5. `0877f0e663129251503bbe0f425244131001114b` — cohort 5/6
6. This final commit — cohort 6/6, exact SHA available in GitHub PR history.

Evidence discipline: cohort assignment is not task completion. The 60 originally pending tasks are split into six verifiable groups, not claimed as implemented merely because a commit lists them. One documentation task (R118-069) was completed in batch 3; the remaining items retain their [ ] state.

### CI correction: stale UDP retirement contract
The restored exact-generation validation intentionally refuses a delayed retiring PID after replacement registration. Updated the pre-existing R118 regression to expect `Err(SocketError::Denied)`, rather than the former `Ok(0)`, with the newer owner’s registry and queue still intact. This is a security-preserving behavior change, not an API compilation workaround. Also marked R118-007 (cached handles remain stale after recovery) and R118-010 (write-time mount probe uncertainty quarantines service) as bounded source/test implementations; these are contingent on exact-final-head CI.

## R118 continuation — production retirement and physical receipt preflight

- Runtime scheduler: the real process exit/fault branch invokes `revoke_task_authority` immediately after successful PCB state transition and **before** fallible page reclamation. Existing `discard` and rollback invoke the same exact-generation idempotent path again. This covers source-level integration of both normal and fault retirement, without claiming that hardware SMP or arbitrary IRQ races are qualified.
- File service: reject forged overflowed/undersized partition extents, missing pinned MBR signature, and changed parent capacity before disk I/O; privileged quarantined-volume recovery similarly checks capacity before probing. Regression tests assert zero reads/writes and preserved quarantine.
- The inherited hardware gates remain OPEN. Exact-head CI regression evidence is required before marking additional source-level items completed.
