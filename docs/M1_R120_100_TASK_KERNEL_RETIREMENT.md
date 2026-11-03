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

## R118 calibrated kernel monotonic dispatch source

- **Source implementation:** `timer::kernel_monotonic_100hz` reads **actual delivered** PIC/PIT or independently calibrated xAPIC timer interrupts. It refuses an inactive timer or zero interrupt progress; APIC is selected when active. Two pure clock-state regressions cover absent progress, active-source precedence, and large tick values.
- **Real network transmit API:** `network::transmit_leased_ipv4_kernel_clock` is available under the timer feature and requires *both* the initial real kernel timestamp and a fresh sample at the NIC dispatch boundary. The common fallible transmission implementation preserves generation-owned route authorization and suppresses physical frame submission if either real clock read fails. A focused regression makes the clock disappear between preparation and dispatch and checks **zero NIC writes**.
- **Scope:** this introduces an actual hardware-timer-backed link transmission API, but the existing R028 QEMU synthetic failover demo still uses its deliberate logical clock. Production NIC call-site integration, proof of monotonicity across timer restart, cross-BSP clock calibration and hardware IRQ delivery are **not** established. R118-041/079 therefore remain unaccepted until the actual live caller uses this path and the correct physical/QEMU evidence is recorded.

## R118 single-BSP filesystem and network lifetime serialization

- **Live kernel production path:** GUI-profile ring-3 filesystem syscall 33 and delegated directory syscall 34 now acquire the same fail-closed BSP/interrupt-masked permit as authenticated UDP syscall 36. This prevents nested/foreign-CPU admission while a FileHandles operation probes the mounted namespace or device.
- **Single guarded generation registration:** `user_runtime::prepare` now admits one permit *before* minting a FileHandles PID generation and keeps it held through coordinator `start_exact` and delegated lease registration, removing the intermediate unguarded registration window.
- **Single guarded retirement:** `revoke_task_authority_with_kind` holds one permit across actual process event issuance, live UDP and capability retirement, directory lease revocation, FileHandles retirement and the post-teardown checks, instead of acquiring/releasing separate permits around only the network portion. This makes the serial order explicit and prevents self-deadlocking nested gate acquisitions.
- **Remaining work:** This does not establish physical SMP or cross-core IPI correctness. The gate still rejects foreign BSP IDs; no AP startup, IRQ-shared network driver, lock-free concurrent filesystem reads or physical IOMMU isolation is qualified. R118-001/003/009/027/028/071/073/074 remain subject to their stated hardware/CI acceptance evidence.

## R118 kernel-owned NIC ingress dispatch (continued implementation)

- `network::authenticated_udp_frame` now performs one bounded, shared Ethernet, IPv4 and UDP checksum/length validation before a datagram may be enqueued; `UdpEndpoints::ingest_ethernet_frame` and `EpochUdpEndpoints::ingest_owned_frame` both use this parser.
- `ProcessRetirement::ingest_kernel_nic_frame` requires an exclusive non-Send `RetirementGate::Permit`. It derives PID and exact generation from the kernel UDP registry using the *validated* destination port and checks current capabilities and socket/registry consistency before mutation. A packet cannot supply its own owner ID. Three regressions cover live peer routing, malformed/wrong-port/foreign-BSP rejection without queue writes, and stale-owner teardown preserving peer traffic.
- This implements a **kernel-only polled NIC dispatch entry point and a future IRQ-facing authority contract** for R118-040/072, not a physical NIC IRQ handler. Physical interrupt routing, NIC-multi-queue IOMMU isolation and cross-CPU lock qualification remain open (R118-028/042/072/074). Even with port-generation gates, delayed network datagrams arriving after rebind cannot carry a process epoch; the hardware ring must be drained/quiesced before publishing a replacement owner.

## R118 injected power-interruption recovery matrix

- Added `r118_power_interruption_fault_matrix` to the real two-sector epoch journal implementation. It injects returned-I/O failure *before write*, *after 64 partial bytes*, *after complete write without acknowledgement*, *at flush*, and *on readback*. Every interrupted commit is denied, preserving the untouched predecessor journal; another test corrupts 1/16/64/104/136/512 bytes of the newer head and verifies the externally sealed minimum epoch prevents rollback.
- This advances source regression coverage for R118-055/077/056/078. **It is in-memory deterministic fault injection, not a real physical unexpected power interruption or boot-sector restoration.** Those hardware gates remain unaccepted and need independent hardware readback, persistent firmware roots and repeatable power-cut logs.

## R118 exact-head review race and branch safety

- `scripts/r118_release_gate.py` now includes its own exact-head evidence unit-test job as a required check, rejects partial review history (100-record first page), rejects bot/unknown approval identities, requires positive human review-count protection with strict status checks, and re-fetches the PR **after** reading checks/reviews to deny a head changed mid-validation. Four targeted Python regressions validate these negative cases.
- This strengthens R118-067/068/070 policy tooling. It does **not** invent a real independent approval: a protected-branch response may be inaccessible to the GitHub app and no human review was recorded at this patch's baseline. Missing authorization and unavailable branch protection remain fail-closed.

## R118 ingress-revision fence across port reuse

- R118-072 implementation continuation: `ProcessRetirement` now maintains a never-recycled checked `ingress_revision`. Privileged PID start/restart, registered UDP bind/close and process exit/fault retirement advance the revision **only after the whole operation succeeds**; stale owners and failures leave it untouched, and overflow refuses mutation.
- The NIC must call `capture_ingress_revision(&permit)` **before polling** and pass that opaque `IngressRevision` to `ingest_kernel_nic_frame`. Before parsing or enqueueing a packet, the dispatch checks that no process/port lifetime changed while the NIC was polling. A frame already received for PID A cannot be silently delivered to PID B after close/rebind, and a peer's packet cannot cross unrelated release/reconfiguration windows.
- Four Rust regressions: stale packet crossing PID recycling, unrelated owner closure while polling, atomic failed-operation revision preservation, and revision-exhaustion denial without partial retirement. This is a concrete single-BSP software RX-ring ownership safety step, **not** proof of real NIC interrupts, DMA queues, IOMMU, APIC IPI, or SMP correctness. Future live physical NIC drivers must capture the revision at RX poll start and drain DMA on rebind.

## R118 cumulative IRQ clock across timer restarts

- The production `timer::kernel_monotonic_100hz` no longer returns PIC/APIC **session-relative counters** that reset to zero during restart or calibration. Instead it reads a single checked, cumulative delivered-IRQ sequence. Actual PIC IRQ0 and calibrated APIC EOI paths advance it; stopped, unconfigured, conflicting, saturated or newly restarted-without-a-fresh-IRQ sources refuse clock authority. The diagnostic PIC/APIC counters remain separately resettable for existing scheduler tests.
- Both host tests exercise APIC→stop→PIC handoff, fresh-session progress, unavailable/conflicting sources and overflow denial. This reduces timestamp rollback/replay exposure for R118-041/079. The hardware-backed network transmit API may now consume cross-session increasing ticks, but the **R028 guest failover demo still uses a deliberately synthetic test clock** and there is no approved multi-CPU timer synchronization or physical interrupt qualification.
- Updated NIC receive regression to distinguish a genuinely **new** packet after rebind from a stale polled packet. A fresh revision token is only safe when the physical driver drains/retags old DMA descriptors. This test does not assert that UDP wire bytes themselves prove a source process generation.

## R118 live virtio NIC per-poll ownership fence

- `network::wait_for_limit_fenced` samples an opaque `IngressRevision` **before every actual `EthernetDevice::poll` call**, and threads it into each successful UDP packet matcher. The R028 physical-QEMU virtio-net demo now uses this shared admission path for primary timeout, backup UDP reply and DNS reply, not only isolated host regressions. The policy service rejects revisions invalidated by process start/retire and UDP bind/close before a packet can be queued under a successor's port.
- `ProcessRetirement::ingest_polled_live_udp_fenced` checks the pre-poll revision before delegating to the existing exact-PID-generation registry and checked Ethernet/IPv4/UDP parser. `ingest_kernel_nic_frame` reuses the same fence and retains the interrupt-masked non-Send BSP permit requirement.
- Scope: the R028 virtio NIC uses synchronous **polling** and a single kernel policy owner; this delivers real QEMU source integration for R118-040/072 but **not** true IRQ/NIC multi-queue ownership, IPI synchronization, physical multi-NIC IOMMU isolation or hardware DMA-ring quiescence. An actual IRQ-driven NIC must validate its hardware descriptor epoch and stop/drain DMA before replacing a port owner.

## R118 hardware-authenticated PID1 launch boundary

- R118-075/076 source implementation: `r114_boot_epoch::AuthenticatedPid1Platform` requires **three distinct hardware-supplied authorities**: verified boot measurement, sealed rollback-resistant floor, and bootloader-verified signed slot. `authorize_verified_pid1` returns a privately constructed read-only `VerifiedPid1Admission` only after the two-copy journal matches all three, rechecks platform authority, and confirms the journal is unchanged. No userland-supplied PID/boot command line, locally fabricated measurement or ordinary EFI writable variable is allowed by the API contract.
- Four real Rust regressions cover stable preprovisioned journal success; zero/invalid measurements and invalid signed slots; a changing sealed floor between checks; and changed media under the verifier. All failures deny a PID1 admission without writes.
- This **does not finish R118-054/075/076**: Limine + the current kernel do not yet implement audited UEFI/TPM provider, immutable root provisioning or boot-to-PID1 attachment. The normal-init profile remains separate and this function is not advertised as protecting a production boot until that provider is installed and integration-qualified.

## R118 actual OVMF guest boot-handoff contract evidence

- The existing `test-r114-epoch` QEMU guest now calls `r114_boot_epoch::authorize_verified_pid1` after writing its real disposable ATA epoch journal and requires a receipt whose selected slot, epoch, journal generation and slot version match the durable readback. The synthetic `QemuBootPolicy` platform supplies the three authority interfaces so the **real no_std guest** (not only host Rust tests) exercises this new fail-closed verification path.
- `tools/test_r114_epoch_qemu.py` requires **both** the new R118 serial handoff marker and the original R114 persistence marker on each positive OVMF cold boot; rejected corrupt-journal negative controls must still leave disk bytes unchanged.
- This is genuine QEMU source-path evidence for the verification algorithm, **not** R118-054/075/076 hardware acceptance: the 0x42 measurement, floor and selected slot are explicitly test-only, not proven by physical UEFI Secure Boot, immutable TPM measurement/PCR or protected monotonic NV storage. Normal PID1 boot remains untrusted until the real platform provider is integrated and attested.

## R118 OVMF hardware-authority negative controls

- The R114 QEMU guest now runs three additional **real OVMF guest** negative cases on the same disposable ATA journal: a protected floor above the observed epoch, a bootloader-selected slot mismatch, and a forged measurement digest. The guest only prints `R118 QEMU PID1 boot authority negative controls PASSED` if all reject; the external QEMU harness requires this marker in addition to the positive handoff and durable epoch markers on **every** cold boot.
- The QEMU platform inputs are explicitly fixed fixtures, not physical firmware trust. These tests strengthen the source-level fail-closed path for R118-054/075/076 without claiming production UEFI/TPM root provisioning or independent security review.

## R118 exact-head checkout across all mandatory integration jobs

- Nine GitHub Actions workflows now checkout the `github.event.pull_request.head.sha || github.sha` commit instead of the default pull-request merge ref; each disables persisted Git credentials and runs `tools/verify_ci_sha.py` before building. The audit includes the kernel ELF/ISO boot, R114 OVMF journal, R028 virtio-net, graphical persistent Files, M1 combined Rust/QEMU regression (both jobs), M2 developer alpha, Orbit QEMU, S0 security and R118 evidence verifier.
- This closes a **source-to-test identity gap** affecting R118-062/063/064/065/066/068: a success check now demonstrates that the exact requested source SHA was checked out, not just a synthetic PR merge SHA. `Roadmap SHA Validation` already checks the live PR head and all checks must still pass on the final integration commit. No QEMU test, no matter how many CPUs configured, qualifies real hardware/IOMMU/TPM/power-cut gates.

## R118 exact SHA custom kernel status publisher

- The long kernel-boot workflow previously checked out an exact PR source commit but its embedded Python status publishers still wrote successful/failed statuses to GitHub's `GITHUB_SHA` (the synthetic PR merge SHA). These publishers now use `INDIANOS_SOURCE_SHA: ${{ github.event.pull_request.head.sha || github.sha }}` uniformly, so each QEMU parity, ring-3, user-image, VMM, paging, PMM, network and kernel acceptance status is attached to the exact source SHA inspected by the job. The standalone checkout verification and live PR head check remain mandatory.
- This strengthens R118-063/068 evidence integrity, but cannot certify a running job as passed or override required independent review, physical hardware/TPM, multi-NIC IRQ/IOMMU or unsafe power-cut qualification.

## R118 full bounded GitHub evidence pagination

- `scripts/r118_release_gate.py` now gathers all exact-head check runs via a **bounded ten-page, 100-per-page** REST traversal. Contradictory `total_count`, non-object/malformed check data, incomplete short pages, changing counts, or a result set beyond capacity fails closed. All required check names must still have successful conclusions on the exact 40-character PR head; a historical failed duplicate blocks the release.
- Independent review history also traverses up to ten GitHub review pages. An exactly full 100-review page must be followed by an actual next page (empty or short) before approval can be accepted. Late `CHANGES_REQUESTED`, stale-head reviews and bot approvals still fail the gate. Bound exhaustion or untrusted API failures deny release.
- Five upgraded/additional Python regression cases cover exact-100 review continuation, late change requests, malformed later review pages, 101 valid check-runs, and changing check-run totals. This advances R118-067/068 release evidence verification **but does not create a human reviewer, protected branch configuration, physical security proof, or post-merge main SHA**.

## R118 live kernel file-and-event retirement transaction

- The real `user_runtime::revoke_task_authority_with_kind` previously ignored **both** an `R114_FILE_EVENTS.enqueue_*_once` failure and `FileHandles.retire_process_batch_kernel` failure. The scheduler now stages the lifecycle notice in a cloned bounded event queue, revokes the exact file-owner generation, then commits the event only when the file transition succeeded. Unexpected failures enter the fatal non-spinning teardown path rather than silently reporting successful scheduler retirement.
- This step occurs after actual UDP endpoint, coordinator, and delegated-directory retirement while the one-BSP `R120_GATE` permit is held, so a PID1 event cannot be observed before its network authority and file handles are gone. A stale/recycled PID returns a harmless no-op and can never revoke a successor.
- Two Rust regressions: a full event queue preserves a still-live worker's file alias and rejects exit until the prior receipt is acknowledged; a zero-capacity queue refuses revocation; stale-generation exit and unsupported event types leave new live authority untouched. This strengthens R118-009/027/073/074 source correctness, but **does not** establish true SMP/IPI atomicity or kernel hardware interrupt testing.

## R118 complete guest-check acceptance set

- The release verifier now requires the exact-head `m2-native-and-guest` and `orbit-ui-preview` GitHub job contexts in addition to the original kernel, security, persistent Files, boot epoch, network, combined regression and SHA identity jobs. These job names were verified from live GitHub Actions for PR #756.
- A separate Python regression proves a missing M2 or Orbit check must block release, regardless of other green results. This closes an evidence-coverage gap for R118-064/066/068; it cannot replace a full kernel boot, an actual independent review, or physical multicore/UEFI/TPM tests.

## R118 BSP-local mount authority tokens

- `fs_handles::SelectedOperationGuard` and `MountAlias` now hold private zero-sized `PhantomData<*mut ()>`, preventing safe Rust from transferring BSP-local filesystem authority proofs to an AP as `Send` or `Sync`. Existing local copies remain supported and revalidation is unchanged.
- Two additional Rust regressions verify stale mount snapshots fail after an exact PID incarnation is retired and rebound, and an existing mount alias fails after kernel-only root narrowing even when its target pathname remains valid.
- This advances the software guard boundary for R118-001/003/014/071, but does NOT implement or certify concurrent SMP remount locks, true two-BSP scheduling, IPI/TLB shootdown, DMA synchronization, or physical multicore tests.

## R118 kernel-timestamped failover retry

- R118-079: `network::transmit_failover_ipv4_kernel_clock` is a new production-oriented no-std API. It rechecks a **prior route receipt and prior dispatch tick**, samples `timer::kernel_monotonic_100hz()` at retry admission and again after frame assembly immediately before NIC submission, denies a stopped/unavailable timer, rollback, non-progressing retry, same-link replay, changed route ownership or expired secondary receipt.
- The actual R028 QEMU virtio-net forced UDP timeout now exercises `transmit_failover_ipv4_checked_clock` with its explicit fixture clock and verifies that the backup link's epoch is current, rather than directly resending with a freshly fabricated independent clock sample. Four Rust regression cases cover same-link denial, time rollback, invalid owner/missing clock, and lease expiry between frame serialization and dispatch.
- This materially advances R118-041/079 source implementation and real QEMU network acceptance. The QEMU policy clock remains deliberately **synthetic**, while production entry point reads the true BSP kernel IRQ clock; this does not prove real hardware clock calibration across multiple CPUs, independent NICs, IOMMU or IRQ/DMA completion. Those remain open.

## R118 one-publication file, UDP and lease retirement

- `fs_handles::FileHandles::staged_copy_kernel` snapshots the exact scheduler-owned file descriptor table, monotonic token allocator, root/mount revision, selected physical receipt and **existing non-resettable write guard** under the single-BSP admission permit. The `WriteGuard` private state is cloned only to stage one transaction, never to hand out new user rights.
- The actual `user_runtime::revoke_task_authority_with_kind` now calls `retire_full_owner_epoch` when a matching live R120 generation exists. It stages FileHandles, PID1 external exit/fault FIFO, R120 capability+UDP+internal notice coordinator, actual UDP endpoint queues, and delegated directory leases. All fallible validations run on staged copies. On full FIFO, out-of-sync endpoint registry or stale incarnation **none of those five live stores change**. On success, all snapshots are committed consecutively without another fallible operation and then checked by the existing QEMU scheduler postconditions. The preexisting legacy test-only path remains available only when there is no matching live R120 owner.
- Two Rust regressions cover a full external PID1 event ring with both PID1/PID2 live UDP sockets and exact file incarnations (failure does not retire either; success after acknowledgment preserves peer authority), plus a malformed divergent socket registry and stale PID replay; a separate FileHandles snapshot regression checks rejected staging cannot reset the live owner or token allocator.
- This meaningfully advances R118-009/027/073/074 and connects the transaction to the real ring-3 desktop teardown. It is still **one BSP with interrupts masked**, not a qualified physically concurrent two-core/IPI/TLB-shootdown implementation. Real SMP lock order and hardware acceptance remain mandatory.

## R118 live QEMU five-table retirement acceptance marker

- The **real** R031 graphical two-boot QEMU scheduler emits `IndianOS: R121 QEMU five-table PID2 retirement transaction PASSED` only after `retire_full_owner_epoch` successfully published the live PID2 FileHandles/UDP/coordinator/PID1-event/delegated-lease transaction. A mismatched coordinator, denied event queue or stale PID cannot emit that marker.
- `tools/test_r031_files_gui_qemu.py` now **requires** that exact guest-generated marker during both disposable-disk cold boots, in addition to R117 per-resource revocation and R120 real ring-3 dual-PID UDP evidence. This makes R118-009/027/064/065/073 acceptance contingent on the new live implementation actually executing, not just host unit tests. SMP hardware and physical two-NIC DMA/IRQ qualification are still open.

## R118 two-incarnation five-table retirement batch

- `user_runtime::retire_full_owner_batch` is an actual no-std bounded (max 2 processes) batch implementation over the same five real kernel stores: `FileHandles`, external PID1 event FIFO, R120 capability/UDP coordinator, live UDP endpoint queues and delegated directory leases. It validates every exact PID generation and duplicate before staging, publishes all five only after all follow-on checks pass, and retains a single-owner wrapper for real scheduler teardown.
- Uses the existing kernel single-BSP IRQ-masked retirement permit, `FileHandles::retire_process_batch_kernel`, `ProcessRetirement::retire_live_{exited,faulted}_batch`, and `LeaseTable::revoke_identity_batch_kernel` instead of issuing sequential per-process releases. No heap allocation, spin waiting or mocked endpoint structure is introduced. A peer never sees only one of two requested retirements.
- Three focused host regressions cover successful two-process fault retirement with exact FIFO event ordering and mount revocation; stale second owner/duplicate PID/invalid PID rejection with both real UDP ports intact; and late event ring capacity failure rolling back all grants and sockets. R118-009/027/073/074 source contract advanced.
- **Qualification limitation:** only one actual BSP currently schedules these syscalls. Multicore IPI acknowledgments, AP context switching, TLB shootdown, physical DMA quiescence and real two-BSP concurrent stress still require hardware-aware implementation and testing; this check is NOT R118-028 done.

## R118 live PID2 stale namespace proof after teardown

- Under the real scheduler's single-BSP permit, the QEMU graphical Files PID2 exit captures a kernel-issued `MountAlias` and `SelectedOperationGuard` while the original exact process generation is still authenticated. Immediately after the same five-table retirement, both proofs MUST reject the old process, even if the pathname is unchanged. A missing initial receipt, retained alias or still-current revision causes fail-closed kernel teardown, not a success marker.
- The actual two-boot `test_r031_files_gui_qemu.py` acceptance harness now requires the guest's `IndianOS: R122 QEMU stale PID2 mount alias and revision guard denied` UART evidence before reporting process retirement complete. The test uses the live Files process's namespace, not a fake unit test or user-provided alias. This advances R118-013/071 software negative controls.
- This real guest proof is still **not** actual concurrent cross-BSP remount/read, a physically selected device swap, arbitrary disk token fuzz over all inputs or hardware IOMMU. Those more demanding R118-013/014/071 obligations remain pending until separately demonstrated.

## R118 QEMU retired PID2 forged file-token corpus

- Added `user_runtime::r123_retired_pid2_file_token_fuzz` to the **actual ring3 graphical Files guest** teardown: after the live PID2 generation has atomically retired and its stale kernel-minted alias has been invalidated, the guest submits 20 independent forged/edge-case descriptor values through `fs_handles::operation_authenticated` for READ/WRITE/cursor/close operations (5 × 20), plus 20 stale mount-alias OPEN attempts. All 120 operations must be rejected before ATA discovery. A denial that modifies the output buffer, process ownership, namespace receipt or security statistics is a fatal error.
- The two-boot QEMU Python harness now requires the guest-only `IndianOS: R123 QEMU retired PID2 file tokens denied before ATA probe` marker, before accepting the R122/R117 teardown markers. This specifically advances the R118-013 stale-token negative test on the genuine ATA-backed, disposable QEMU Files guest; unlike an isolated fake-disk unit test, it runs with the real scheduler and 128-sector scratch volume.
- Limit: 20 adversarial tokens × six operations is bounded deterministic fault-injection coverage, not an exhaustive proof over all 64-bit tokens or a physically concurrent selected-volume hot swap. Exact R118-013 physical remount/stale-token and full fuzz coverage still needs additional qualification.

## R118 real QEMU kernel NIC ingress path wiring

- R118-072 software integration: the **actual emulated legacy virtio-net** UDP echo and DNS RX closures now capture the live process coordinator's ingress revision under a single-BSP `RetirementGate::Permit` before each device poll and dispatch the packet through `ProcessRetirement::ingest_kernel_nic_frame`, rather than the prior caller-PID-asserted `ingest_polled_live_udp_fenced` shim. The stricter function validates packet checksums/framing, resolves the receiving process incarnation from the kernel's destination-port registry, rechecks the captured revision and exact live owner, then adds data to its real socket FIFO. A caller-supplied PID is not trusted as packet authority.
- The production QEMU network integration requires `IndianOS: R124 QEMU permit-fenced kernel NIC UDP/DNS ingress PASSED` after both UDP and DNS responses have been received from the actual NIC. The R028 network test harness now insists on that guest marker. This covers software authorization for delayed packets after a port generation changes.
- **Not physical IRQ qualification:** this legacy virtio-net backend is single-CPU, polled with interrupts disabled, no MSI/MSI-X delivery, two-NIC IOMMU or DMA/IRQ quiescence, and the gate's CPU ID 0 is a trusted QEMU test assumption. Actual R118-042/072 physical IRQ completion remains open.

## R118 Limine MP bootloader enumeration and parked AP foundation

- The Rust-owned `limine_requests.rs` now requests the actual revision-6 Limine MP response using the upstream `LIMINE_MP_REQUEST_ID` and the exact `MpRequest/MpResponse/MpInfo` C ABI, validates at most 64 CPUs, aligned and nonnull bootloader pointers, unique processor and local-APIC identifiers, exactly one declared BSP, and a NULL `goto_address` for every secondary CPU. The parser is used **only** during the trusted BSP initial handoff before bootloader memory reclamation; it never treats user input as a boot protocol response.
- The real R028 virtio-net QEMU smoke now boots with **two virtual CPUs** (`-smp 2`) and requires the kernel-generated `R125 QEMU two-CPU Limine MP inventory validated; AP parked` marker. Four host regressions include exact wire offsets, 2-CPU success, duplicate/missing/invalid counts and a wrongly prestarted AP. The network GitHub workflow runs these MP regressions explicitly.
- **Important boundary:** Limine keeps the AP parked until a kernel writes its `goto_address`. IndianOS deliberately does NOT write that address yet: it needs dedicated AP stacks, per-CPU GDT/IDT/TSS and scheduler ownership, APIC IPI acknowledgement, synchronized TLB shootdowns, cache/DMA fencing and multi-CPU deadlock tests. Thus R118-014/027/028/073/074 remain OPEN. Two CPUs being enumerated is not equivalent to two independent CPUs running IndianOS safely. This source change must pass the new exact-head QEMU check before it is counted as verified.

## R118 isolated real AP execution acknowledgement

- R126 opt-in QEMU-only feature `test-r118-ap-ack`: when the BSP validates exactly two parked Limine vCPUs and BSP LAPIC 0, it writes one secondary `goto_address` to an AP-only Rust callback, which executes on the secondary CPU, disables interrupts, obtains **its own x86 CPUID local APIC ID**, publishes an atomic one-time acknowledgement, and parks in CLI/HLT. The BSP waits a bounded number of spin iterations and then halts **before PMM initialization or user programs**, so no AP uses bootloader memory after reclamation.
- New `make test-r118-ap-qemu` builds a separate freestanding image, runs OVMF with two real QEMU vCPUs and requires serial evidence of the MP inventory and AP callback on the same cold boot. New separate GitHub workflow `r118-two-cpu-ap-qemu` checks exact source SHA, Rust MP host tests, freestanding compilation and actual AP guest entry. The read-only release verifier now makes this exact-head guest job mandatory.
- Still **NOT R118-014/027/028/073/074 accepted**: no dedicated per-AP stacks, GDT/IDT/TSS, SMP locks, IPI or TLB shootdown, real network IRQ/DMA quiescence, concurrent scheduler teardown, physical multi-socket CPU or multiprocessor stress. The isolated test only proves controlled AP entry, actual CPUID and a single atomic acknowledgement. No physical CPU or independent human security review is claimed.

## R118 real secondary-CPU BSP authority-denial stress

- R127 builds on real Limine/QEMU AP execution: BSP registers CPU0 as the exclusive kernel `RetirementGate` owner; the secondary CPU verifies its **own** CPUID/LAPIC ID 1 and attempts to acquire that same gate **8,192 times**. Every attempt must yield `WrongCpu` without blocking or mutating state. The bootstrap CPU repeatedly admits/releases its own non-Send permit concurrently, then verifies the AP count and proper atomic acknowledgement before halting.
- The exact-head QEMU AP harness now requires `IndianOS: R127 QEMU real AP denied BSP retirement gate 8192 times` immediately after the R126 AP entry proof. This is actual two-vCPU execution and a bounded nonblocking cross-CPU authority-denial stress, rather than host threads pretending to be APs.
- R118-001/027/073/074 still **open**: this gate permits only BSP mutation; there is no actual AP task scheduler, FileHandles or network operation on CPU1, no AP GDT/IDT/TSS setup, no IPI/TLB acknowledgement or physical cross-socket proof. Protecting BSP-only access is a foundation, not two-BSP transaction qualification.

## R128 active PID2 forged-handle QEMU admission

- Added a **live** PID2 negative test to the actual graphical Files process before its five-table retirement while the R120 BSP file-service admission permit is held. `FileHandles::r128_audit_active_forged_handles` checks 20 guaranteed-unissued descriptor values (`next .. next+19`) against 5 read/write/seek/close/truncate operations, for **100 authenticated, active-generation malformed file syscalls**. Every result must be `InvalidName` before the genuine ATA controller is probed; all results preserve the caller's output buffer, actual handle table entries/offsets, descriptor allocator, roots, namespace and volume revisions, write budget statistics and process generation.
- The two-boot real QEMU Files acceptance harness requires the new **kernel-emitted** `IndianOS: R128 QEMU active PID2 forged file tokens denied before ATA probe` marker, in addition to the existing R123 retired-owner token corpus and R122 stale alias invalidation. This is genuine full-syscall preflight under a live PID2 identity, not only a simulated after-retirement failure. R118-013/071 source and live QEMU evidence advanced.
- This still is NOT a hardware hot-swap, exhaustive token fuzz, true concurrent AP filesystem lookup, or physical storage controller test; R118-003/013/014/071 acceptance stays open beyond this bounded guest proof.

## R129 live retirement permit on every five-table publication

- R120-013 source hardening: `user_runtime::retire_full_owner_batch` and `retire_full_owner_epoch` now require a borrowed, non-Send `retirement_gate::Permit` in their Rust signatures. The live scheduler passes its already-held R120 gate permit through the five-store preflight, copy and publication. The wrapper forwards the **same permit**, without a nested gate acquisition.
- All five atomicity/rollback regression tests obtain a non-spinning BSP permit before invoking either transaction. The interface prevents accidental calls without a permit; cross-CPU admission remains fail-closed.
- This is NOT true SMP file/UDP locks or IPI synchronization. R120-013 and 042 retain their real multicore acceptance prerequisites.

## R130 real NIC frame replay after PID/UDP generation recycling

- R120-026 source/QEMU continuation: the real `network::run_demo` virtio-net path captures the **actual Ethernet UDP reply frame**, local MAC and pre-poll `IngressRevision` only after authenticating it through `ProcessRetirement::ingest_kernel_nic_frame`. After the old process exits, the same PID and UDP destination port are rebound to a **new generation**.
- A replay of the saved real NIC frame using the old descriptor revision **must fail with `RetirementError::Sockets(SocketError::Denied)`**, even though the recycled PID now legitimately owns the destination port. The kernel asserts the recycled owner's FIFO is still empty, then emits `IndianOS: R130 QEMU stale NIC frame denied after PID/UDP epoch recycle`. The R028 end-to-end network harness explicitly requires this marker along with IPv4/UDP/DNS/TCP tests.
- This tests authenticated frame-to-epoch correctness under actual QEMU virtio RX and cooperative retirement, not physical NIC MSI/MSI-X, hardware interrupt-handler generation fencing, DMA retirement barriers or multi-NIC IOMMU isolation. R120-026/028 remain open for physical IRQ qualification.

## R131 real two-vCPU ordered lifecycle receipt rendezvous

- Added `rust/kernel_core/src/smp_lifecycle_fence.rs`: a no_std allocation-free single-BSP / one-AP release-acquire sequence-and-generation mailbox, which permits only one outstanding request, rejects wrong CPUs, rejects stale/zero generations, denies stale acknowledgements and refuses sequence overflow. Rust host tests cover ordering, replay, foreign CPU and 64 concurrent thread exchanges.
- Actual Limine/QEMU two-vCPU guest test executes **64 BSP-to-secondary-CPU lifecycle receipt exchanges** on the real secondary CPU, after 8,192 denied BSP permit attempts. The AP acknowledges only consecutive generations, the BSP cannot publish the next request before seeing an exact-sequence ACK, and both cores halt before PMM initialization. The real guest emits `IndianOS: R131 QEMU 64 ordered cross-CPU lifecycle receipts acknowledged`, mandatory in the QEMU AP harness.
- **Acceptance boundary:** R131 proves real shared-memory atomic ordering across two QEMU vCPUs. It does not perform APIC IPI delivery, per-CPU TLB invalidation, per-CPU GDT/IDT/TSS, actual production multicore task execution, physical IRQ/DMA fencing or independent hardware review. `R120-013` and `R120-042` remain open for actual SMP scheduler and interrupt qualification; no fake completion checkmarks.

## R132 x2APIC guest qualification blocked by QEMU TCG

- Follow-up R132 implementation added an isolated x2APIC IPI interrupt gate, privileged ICR write and EOI handler and **correctly failed closed** when the test runner exposed no `CPUID.01H:ECX.x2APIC` support. Explicit `qemu64,x2apic=on` also failed: the actual runner logged `TCG doesn't support requested feature: CPUID.01H:ECX.x2apic [bit 21]` and the guest logged `FATAL R132 AP CPUID missing x2APIC`.
- Because the required QEMU runner cannot execute x2APIC, the unqualified experimental R132 code and marker were removed from the mandatory boot path. The R131 64-exchange, real two-CPU release/acquire protocol and R127 8,192 wrong-CPU denials remain required and were previously green. No test assertions were disabled or fake success inserted.
- **R120-042 is still OPEN:** next qualification must run a real IPI + EOI + per-CPU TLB-shootdown test on confirmed x2APIC-capable QEMU/KVM/physical CPU, followed by production scheduler retirement integration and independent review. The TCG limitation is environment evidence, not a claim that the kernel supports all hardware.

## R133 R120-013 production retirement gate provenance

- Production scheduler five-table exit/fault retirement now requires **two proofs**: a borrowed non-Send `RetirementGate::Permit` and a reference to the **actual production `R120_GATE`**. Before staging or cloning any live file/network/capability/lease/event state, the retirement function checks `permit.issued_by(expected_gate)`, comparing the exact gate object's address rather than accepting any locally constructed permit.
- The four existing single-owner and three batch regression call sites pass their own gate; actual ring3 scheduler passes `&R120_GATE`. A new test checks that a valid permit issued by a *different* gate with the same BSP CPU ID is refused without altering either process, port, event or lease; a subsequent permit from the correct gate retires both owners.
- This closes an accidental authorization-by-unrelated-gate route in the R120-013 *single-BSP implementation*. It does not establish shared mutable SMP kernel tables, real physical IRQ NMI reentrance safety or APIC IPI/TLB lifecycle ordering; R120-013/042 remain formally open.

## R134 NIC software-IRQ admission and drain fencing

- Added `nic_rx_fence.rs` with a no_std, per-device, bounded **non-spinning** IRQ-side `try_admit` and RAII in-flight reference. A per-device `begin_quiesce` stops new admissions, `drained` refuses publication while any handler remains, and `finish_rebind` advances a checked generation only after software handlers drain. Wrong devices, unrelated fence handles, stale receipts and rollovers fail closed.
- `ProcessRetirement::ingest_nic_frame_device_fenced` requires BOTH the per-device live `RxPermit` and existing non-Send BSP retirement permit, rejecting wrong-device and stale/quiesced permits before parsing, owner lookup or UDP FIFO access. Host tests include actual cross-thread in-flight handler/quiesce ordering.
- **Device safety limitation:** software RX draining is not a hardware DMA barrier. A production NIC driver must still mask MSI/MSI-X, stop PCI bus mastering, poll DMA completion/IOMMU invalidation, prevent late descriptor reuse, and test physical multiple NICs. This source is a foundation for R120-026/028 only; both are formally open pending physical IRQ/IOMMU qualification.

## R135 live QEMU polled NIC drain-before-recycle and foreign-device regression

- The actual R028 guest's virtio UDP and DNS receive path now uses `NicRxFence::try_admit` and `ProcessRetirement::ingest_nic_frame_device_fenced` on every authenticated packet. On process teardown the single-BSP guest explicitly quiesces the software NIC receive guard, requires zero active handlers, commits exact owner/UDP retirement, recycles the PID and destination port, advances the device RX epoch, and rejects replay of the **original captured real virtio Ethernet reply** against the new owner. A guest-generated marker `R135 QEMU NIC RX drained before UDP owner recycle PASSED` is mandatory in `tools/test_r028_network_qemu.py`.
- Added source regression with two different NIC fence objects having the **same numeric device ID**: a permit from the wrong fence must not enqueue; even a previously valid permit is denied after quiesce; stale pre-retirement UDP revision is refused after drain/rebind with no recycled-owner FIFO mutation.
- The last R134 change is strengthened with atomic CAS on device-epoch rebind to refuse concurrent stale controller attempts. **This is still a polled QEMU virtio NIC; no real MSI, IOMMU, DMA halt or cross-physical-NIC validation is implied.** R120-026 and -028 remain open until those external/driver requirements are proved.

## R136 exact BSP kernel-gate provenance on device-fenced UDP ingress

- The R134/R135 device-fenced NIC ingress API now also requires an explicit trusted `RetirementGate` reference. It verifies that the passed non-Send BSP `Permit` was issued by **that exact gate instance**, rejecting unrelated gates before parsing or queue writes. The actual QEMU virtio client passes its unique `ingress_gate`; the host regression submits a foreign gate with the same BSP CPU number and confirms the live owner's UDP FIFO is unchanged.
- This strengthens R120-013 and R120-026's software admission boundary without claiming real MSI/MSI-X, physical multi-NIC IOMMU isolation, or production APIC scheduler safety. The complete R120 acceptance ledger remains 65/70 pending real SMP/hardware/review gates.
