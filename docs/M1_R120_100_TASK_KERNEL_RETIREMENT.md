# IndianOS R120 — bounded atomic process retirement, 100-item checklist

**Baseline main:** `654a7a35a66307f25d7ed759a2a0ba5ee8f3cfcf`. **Delivery:** one pull request with successive implementation/fix commits and five logical workstreams (not five independently launched AI agents).

**Scope:** 70 new, individually named acceptance tasks plus 30 inherited R118 items in the primary 100 checklist. The remaining 21 inherited R118 items and 15 still-open R119 items are retained below. Each `[x]` means *source implementation or focused host test authored in this commit*, not CI pass, real scheduler integration, QEMU qualification, physical hardware acceptance, or merge.

The added Rust `no_std` coordinator stages and atomically commits capabilities, UDP policy-grants, and one-time exit/fault receipts under a single-owner assumption. It is **not** yet integrated with the live scheduler, actual network socket table, SMP locking or physical IRQ paths.

## Primary 100 task checklist — 70 R120 + 30 inherited

### Workstream 1 — Capability incarnation contract

- [x] R120-001 — Add read-only active generation lookup
- [x] R120-002 — Return no generation for retired owner
- [x] R120-003 — Reject stale generation before grant
- [x] R120-004 — Use generation-stamped owner receipt
- [x] R120-005 — Keep generation advance on PID restart
- [x] R120-006 — Reject zero PID on start
- [x] R120-007 — Keep independent capability table staging
- [x] R120-008 — Preserve peer capability grants during exit
- [x] R120-009 — Prevent stale ticket replay after retirement
- [x] R120-010 — Prevent generation spoof on authorization
- [x] R120-011 — Keep grant rights enforcement
- [x] R120-012 — Handle capability table capacity limits
- [ ] R120-013 — Prevent lock-free access across cores
- [x] R120-014 — Wire lifecycle authority to live scheduler

### Workstream 2 — Generation-owned UDP policy

- [x] R120-015 — Stage bounded UDP registry clone
- [x] R120-016 — Bind only an authenticated live incarnation
- [x] R120-017 — Require exact-generation UDP authorization
- [x] R120-018 — Revoke multiple UDP ports on process exit
- [x] R120-019 — Preserve unrelated peer ports
- [x] R120-020 — Reject foreign-generation retirement
- [x] R120-021 — Do not partially release on invalid later owner
- [x] R120-022 — Reject duplicate PID in a retirement batch
- [x] R120-023 — Prevent stale process from binding after restart
- [x] R120-024 — Safely handle zero-capacity UDP registry
- [x] R120-025 — Wire retired grants to actual network socket table
- [ ] R120-026 — Enforce interrupt ingress generation ownership
- [x] R120-027 — Test two ring3 process isolation in QEMU
- [ ] R120-028 — Qualify physical multi-NIC IRQ/IOMMU safety

### Workstream 3 — Exit/fault lifecycle receipts

- [x] R120-029 — Clone bounded FIFO transactionally
- [x] R120-030 — Emit generation-qualified exit notices
- [x] R120-031 — Emit generation-qualified fault notices
- [x] R120-032 — Deduplicate exact fault incarnation
- [x] R120-033 — Separate Faulted and Exited kinds
- [x] R120-034 — Reject invalid fault process ID
- [x] R120-035 — Reject zero fault generation
- [x] R120-036 — Support zero-capacity fail-closed events
- [x] R120-037 — Preserve one-time FIFO acknowledgement IDs
- [x] R120-038 — Prevent stale acknowledgement replay
- [x] R120-039 — Preserve event FIFO across batch failure
- [x] R120-040 — Reject full queue without publishing retirement
- [x] R120-041 — Wire actual scheduler fault production path
- [ ] R120-042 — Qualify cross-core lifecycle IPI ordering

### Workstream 4 — Cross-table transaction

- [x] R120-043 — Create no_std process-retirement coordinator
- [x] R120-044 — Store capability, UDP and event tables as one kernel policy owner
- [x] R120-045 — Authenticate PID and generation for every group member
- [x] R120-046 — Reject duplicate PID before commit
- [x] R120-047 — Stage capability retirement
- [x] R120-048 — Stage UDP release
- [x] R120-049 — Stage lifecycle notification queue
- [x] R120-050 — Publish all three only after full success
- [x] R120-051 — Return exact counts of retired resources
- [x] R120-052 — Support two-owner atomic exit batches
- [x] R120-053 — Rollback on late stale owner
- [x] R120-054 — Rollback on stale UDP grant
- [x] R120-055 — Rollback on event queue backpressure
- [x] R120-056 — Connect coordinator to production kernel scheduler

### Workstream 5 — CI, delivery and hardware gates

- [x] R120-057 — Add cross-table successful-exit regression
- [x] R120-058 — Add stale-later-owner rollback regression
- [x] R120-059 — Add full-event-queue rollback regression
- [x] R120-060 — Add foreign-generation UDP rollback regression
- [x] R120-061 — Add fault retirement and PID recycling regression
- [x] R120-062 — Add duplicate and zero-capacity regression
- [x] R120-063 — Add fault receipt deduplication regression
- [x] R120-064 — Add invalid/zero fault receipt regression
- [x] R120-065 — Retain a 100-task acceptance ledger
- [x] R120-066 — Retain inherited pending hardware gates
- [x] R120-067 — Run exact-head Rust host regression CI
- [x] R120-068 — Run optimized freestanding no_std CI
- [x] R120-069 — Run graphical persistent Files and ring3 UDP QEMU
- [ ] R120-070 — Obtain independent review and verified main merge

### Inherited 30 R118 items (unverified, not claimed complete)

- [ ] R118-001 — Audit cross-BSP mount revision synchronization
- [ ] R118-003 — Reject stale mount revision after concurrent replace
- [ ] R118-009 — Prove deterministic deadlock-free file-owner retirement
- [ ] R118-013 — Exercise selected-volume stale-token fuzz in QEMU
- [ ] R118-014 — Qualify true SMP concurrent remount/lookup
- [ ] R118-027 — Run two-BSP scheduler revocation stress
- [ ] R118-028 — Qualify real multicore IPI and teardown safety
- [ ] R118-040 — Prove foreign UDP port close denial in QEMU
- [ ] R118-041 — Integrate calibrated kernel monotonic source instead of test clock
- [ ] R118-042 — Qualify ingress IRQ under multiple physical NICs
- [ ] R118-054 — Wire production bootloader trust anchor
- [ ] R118-055 — Qualify real unsafe physical power cut
- [ ] R118-056 — Verify hardware boot-sector restoration
- [ ] R118-062 — Require freestanding optimized no_std build
- [ ] R118-063 — Require exact-head host Rust unit suite
- [ ] R118-064 — Require two-boot persistent graphical Files QEMU
- [ ] R118-065 — Require real two-process ring3 port-ownership QEMU
- [ ] R118-066 — Require full QEMU network and boot matrix
- [ ] R118-067 — Require independent kernel/security review
- [ ] R118-068 — Confirm CI exact integration SHA
- [ ] R118-070 — Publish protected merge and post-merge main evidence
- [ ] R118-071 — Inherited R117-071: Inherited R116-071 — Inherited R115-074 — Reject concurrent mount replacement during path lookup
- [ ] R118-072 — Inherited R117-073: Inherited R116-073 — Inherited R115-078 — Enforce port-generation checks at network ingress IRQ
- [ ] R118-073 — Inherited R117-076: Inherited R116-076 — Inherited R115-084 — Audit lock ordering of file/network lease teardown
- [ ] R118-074 — Inherited R117-077: Inherited R116-077 — Inherited R115-085 — Prove no scheduler teardown deadlock in SMP configuration
- [ ] R118-075 — Inherited R117-078: Inherited R116-078 — Inherited R115-086 — Use trusted boot observer in production PID1 boot path
- [ ] R118-076 — Inherited R117-079: Inherited R116-079 — Inherited R115-087 — Provision immutable boot measurement root in bootloader
- [ ] R118-077 — Inherited R117-080: Inherited R116-080 — Inherited R115-088 — Verify boot journal under actual unsafe power cut
- [ ] R118-078 — Inherited R117-081: Inherited R116-082 — Inherited R115-090 — Qualify physical boot-sector restoration on hardware
- [ ] R118-079 — Inherited R117-082: Inherited R116-083 — Inherited R115-092 — Read a kernel monotonic timestamp at transmit and retry

## Outstanding obligations beyond the first 100

### Remaining 21 R118 items

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
- [ ] R118-93 — Exact-head Rust host suite passes
- [ ] R118-94 — Exact-head optimized no_std build passes
- [ ] R118-95 — QEMU persistent Files cold reboot passes
- [ ] R118-96 — Real ring3 two-process UDP QEMU acceptance passes
- [ ] R118-97 — QEMU boot-epoch failure matrix passes
- [ ] R118-98 — All required workflows green on same final SHA
- [ ] R118-99 — Independent code review and branch protection complete
- [ ] R118-100 — Publish verified merged-main SHA and physical-hardware evidence gaps

### Fifteen R119 tasks not accepted in merged PR #754

- [ ] R119-014 — Qualify canonical file-handle write guard on QEMU persistent desktop
- [ ] R119-027 — Wire group retirement through real network socket table
- [ ] R119-028 — Qualify two ring3 processes and foreign-close denial in QEMU
- [ ] R119-041 — Wire group retirement to scheduler normal-exit and fault paths
- [ ] R119-042 — Qualify real SMP lock ordering of multi-owner capability teardown
- [ ] R119-055 — Wire group event path to live scheduler teardown
- [ ] R119-056 — Qualify cross-core IPI and IRQ scheduler exit delivery
- [ ] R119-063 — Run exact-head full Rust unit tests
- [ ] R119-064 — Run optimized freestanding no_std build
- [ ] R119-065 — Run two-boot persistent graphical Files QEMU
- [ ] R119-066 — Run ring3 UDP ownership QEMU
- [ ] R119-067 — Run kernel ELF/ISO serial boot evidence
- [ ] R119-068 — Require independent human kernel/security reviewer
- [ ] R119-069 — Require all mandatory final-SHA CI checks to pass before merge
- [ ] R119-070 — Publish merge SHA and unverified physical-hardware gates

## Acceptance and explicit limitations

- New source-scoped R120 checks: **65/70**, remaining **5/70**. These are granular API or regression acceptance items, not 57 separately deployable OS features.
- Inherited R118 hardware/CI/review gates: **0/51 validated here**; merged PR #754 also retained 15 R119 acceptance tasks that require live integration or evidence.
- Added Rust tests: six coordinator tests and two fault-receipt tests; they must pass CI on the exact PR head.
- Required release matrix: host Rust suite, optimized freestanding `no_std`, two-boot persistent graphical Files QEMU, ring3 two-process UDP QEMU, boot ELF/ISO, and independent kernel/security review. Verify checks against the exact final SHA and merge only when project policy allows.
- Physical SMP, cross-core IPI, hardware IRQ ownership, DMA/IOMMU isolation, immutable firmware measurement root, actual unexpected power loss and disk repair remain unverified; QEMU passes cannot substitute for hardware.
- Do not interpret this ledger as 70 completed tasks in one commit or 100 finished OS features. The source-correctness flags are intentionally narrower.


## R120 pending-task continuation — actual code (same PR)

- **R120-025 source-implemented:** Real `network::EpochUdpEndpoints` now exposes an exact kernel registry consistency check; `ProcessRetirement` offers `bind_live_udp`, `retire_live_exited_batch` and `retire_live_faulted_batch` with copy/stage/commit atomicity across the real datagram queue, policy registry, capability epoch, and event FIFO. It checks exact PID-generation identity, prevents stale/foreign owner teardown, preserves peer messages, and denies divergent tables without mutations. Four added host regressions cover peer preservation, late stale members, full queue rollback, registry mismatch, and fault receipts. This new API is **not yet called from the production ring3 scheduler**; R120-056 remains open.
- **R120-041 source-implemented:** The existing ring3 scheduler normal-exit and fault branch now selects precise `Exited` or `Faulted` event production. Rollback/retry paths still default to `Exited`; only authenticated completed faults produce `Faulted`. Existing real scheduler authority-retirement host test now checks fault receipt generation and replay safety. This does **not** qualify cross-core IPI delivery (R120-042).
- Exact-head host unit tests, optimized `no_std`, QEMU, and independent review must rerun on the *new* head before merge. All R118/R119 physical and pending acceptance gates remain retained.

## R120 polled virtio-net lifecycle ingress continuation

The R028 QEMU `run_demo` kernel network path now obtains a generation-stamped
owner from `ProcessRetirement` rather than constructing a standalone UDP
registry. Its live UDP bind, polled receive ingress, read-back and retirement
operations all run through coordinator APIs; the finished QEMU DNS/UDP test
checks that the owner was retired and its grant was actually removed. Two Rust
regressions reject fabricated/retired owners before frame parsing, prevent
retired PID-grant reuse after restart, and preserve packet/output state.

**Scope:** This is a genuine virtio-net *polled* guest receive path, not a
production NIC interrupt handler. R120-026 (interrupt ingress ownership)
therefore stays unchecked until an actual IRQ-driven path is wired and tested.
R120-027 (two ring3 QEMU processes), R120-013/042 (SMP/IPI), and
R120-014/056 (live scheduler coordinator integration) stay unchecked.
Exact-head CI must be rerun on any resulting new commit. No physical hardware
acceptance or independent human review is implied.

## R120 live scheduler integration implementation

- **R120-014:** the privileged `user_runtime::prepare` path now registers the exact generation already minted by `FileHandles::bind_process` into the process-retirement policy. The globally monotonic filesystem generation must never be approximated by a per-PID counter; stale or repeated registrations are denied.
- **R120-056:** the existing ring-3 syscall 36 bind/send/receive/close operations now use the same actual `ProcessRetirement` coordinator and real `EpochUdpEndpoints` socket table. Exact authenticated scheduler exit and fault paths retire those registered incarnations transactionally before file-handle and page-table reclamation. An old delayed task cannot revoke a newer generation. Retiring policy receipts are separately drained; the preexisting FileHandles-authenticated PID1 notification queue is retained for userspace ABI compatibility.
- Added generation allocator tests and live socket close/send/peer preservation tests. The test-created legacy registry fallback is only for synthetic host fixtures that do not register a coordinator owner; it is not the production ring-3 syscall path.
- **Not evidence of SMP lock exclusion:** This runtime remains single-BSP; IRQ/SMP/physical hardware and actual multicore scheduler gates remain open. QEMU and no_std must pass at the final SHA before merging.

## R120 real two-process QEMU acceptance enhancement

The privileged ring-3 scheduler now caches the actual FileHandles-minted
generations of PID1 and PID2 after successful process preparation. After both
processes terminate, it verifies the lifecycle coordinator has no active
generation for either PID, both real FileHandles owners have been revoked,
neither old generation can access its UDP endpoint, and all live UDP socket
bindings agree with the policy registry. Only then does it emit
`IndianOS: R120 QEMU ring3 dual-PID UDP epoch retirement PASSED`.
The existing **two-boot graphical Files** guest test now requires that
kernel-emitted marker, in addition to existing real PID1/PID2 UDP PING/PONG,
forged foreign-close denial, and actual keyboard, framebuffer, and disk-readback
checks.

R120-027 is **implemented but remains unchecked until the exact new SHA's
graphical Files QEMU integration passes**. Host tests do not substitute for
guest acceptance. The R120-013, -026, -028, -042 SMP/physical IRQ/IOMMU
gates and -070 independent review/merge remain unqualified.

## R120 exact-code-SHA CI acceptance (2026-11-03)

Source commit `0883993c27cbceab596f82cad62ffd5e6cba4369` passed GitHub Actions **combined-regression** (including real runtime module host regressions), **durable-files-desktop**, and that workflow's explicit **optimized freestanding kernel-core build** and **real two-boot graphical Files, ring-3 UDP and persistent scratch disk QEMU** steps. The graphical guest harness required the new kernel-produced dual-PID exact-generation retirement marker, not merely a mocked or log-only unit test. The **network**, **security regression** and **SHA validation** checks also passed on this source commit.

The four completed evidence gates R120-027, R120-067, R120-068 and R120-069 are checked against the referenced source SHA. **This ledger update creates a new commit SHA, so final-PR-head CI must run again before merge.** Any changed source would also require a fresh qualification. R120-013 (real SMP access control), R120-026 (NIC IRQ-driven ingress), R120-028 (physical multi-NIC IOMMU), R120-042 (cross-core lifecycle IPI), and R120-070 (independent human review, branch protection and merge SHA) remain open. Do not equate QEMU coverage with physical hardware qualification.
\n## R118 inherited source-hardening continuation (current PR)\n\n- **R118-009 / R118-073 (partial implementation):** the real `FileHandles::retire_process_batch_kernel` now retires authenticated owners in canonical PID order after a full batch preflight. This removes caller-order dependence and avoids acquiring nested locks in this single-BSP path. Three focused regression tests exercise reversed-order batches, 64 PID-generation reuses with stale later members, and namespace guard invalidation through root narrowing and teardown.\n- **R118-001 / R118-003 / R118-071 (partial source coverage):** selected-operation guard, alias and namespace-revision checks remain fail-closed on metadata replacement. The new test exercises revision invalidation, but **does not simulate concurrent physical CPUs or interrupt-time mount mutation**.\n- **Evidence discipline:** no inherited checkbox is marked accepted merely because a code path or test exists. A new commit changes the PR head: Rust host, optimized no_std, real QEMU network, two-boot persistent graphical Files, boot and security checks need to complete on this exact head. SMP, NIC IRQ/IOMMU, firmware trust, real power loss, independent human review and protected merge remain unqualified.\n
## R120 single-BSP atomic ownership gate (source implementation)

The live ring-3 UDP syscalls, privileged process preparation, scheduler normal-exit/fault retirement, session socket reset and QEMU terminal verification now use a non-spinning Acquire/Release atomic ownership permit scoped to the BSP APIC ID with CPU interrupts masked. Cross-CPU, nested and IRQ-enabled access is rejected; privileged retirement failure halts the guest rather than reclaiming frames with uncertain active UDP authority. The unit-tested `retirement_gate` rejects unbound, wrong-CPU, interrupt-enabled and nested accesses. This is defensive **single-BSP** ownership control, **not real SMP implementation**. The current legacy 8-bit APIC-ID identity is not sufficient to qualify x2APIC multi-socket topology. No CPU-startup/IPI, interrupt-driven NIC ingress or real IOMMU/DMA safety is claimed. R120-013, -026, -028, -042 and -070 remain pending hardware and independent review evidence. Re-run exact-final-SHA CI; previous QEMU results are not final-head evidence.

## R118 selected-volume fuzz and host admission stress

- **Source/test additions (not hardware acceptance):** `r118_selected_volume_stale_handle_fuzz_across_128_pid_recycles` drives the real authenticated selected-volume API through 128 PID2 generations, including both stale and recycled owner/token combinations; asserts denial and *zero disk reads* for all stale attempts. `r118_selected_mount_revision_swap_rejects_old_alias_before_disk_probe` rejects stale namespace aliases after a trusted revision transition before any sector I/O. These cover source-level portions of R118-003/013/071, **not real concurrent QEMU guest mutation or cross-BSP races**.
- **Atomic admission stress:** two host-thread tests in `retirement_gate.rs` reject non-BSP claims (8192 attempts) and challenge the non-spinning kernel permit with concurrent host threads. This is source-level assistance toward R118-009/027/028/073/074, but **does not establish actual multi-CPU kernel scheduler, cross-core interrupt handling or deadlock freedom on SMP hardware**.
- These tests are integrated into the full Rust host suite. Exact-current-head host/no_std/QEMU CI, branch review and physical evidence still govern formal acceptance. The inherited R118 acceptance checkboxes remain unchecked until their exact required evidence is verified.
