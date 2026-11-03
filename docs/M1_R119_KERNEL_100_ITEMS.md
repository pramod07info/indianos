# IndianOS R119 — single-commit kernel hardening acceptance ledger

**Baseline:** `d288358d1da51b7ee190853daadaef0c1b69a819` (`main` at branch creation). **Five logical workstreams**, not five independent agent instances.

**Requested scope:** 70 new R119 tasks and ALL 51 outstanding R118 tasks (121 unique tasks). To preserve the requested 100-item report, 70 new items and the first 30 inherited items make up the primary checklist; 21 additional inherited tasks appear in a clearly visible appendix below. There is no assertion that 100 items have been implemented.

**Checkbox contract:** `[x]` = source change or focused unit test authored in the R119 commit, NOT CI-passed, end-to-end QEMU-qualified, independently reviewed, physical hardware tested, or merged. The retired-policy APIs do not yet guarantee that live scheduler, UDP socket, and capability services are wired together. These new checks are small-granularity source acceptance items, not independent fully delivered OS features.

## Primary 100-item checklist: 70 new + 30 carry-forward

### W1: Filesystem and namespace identity
- [x] R119-001 — Normalize write-guard identity for path comparisons
- [x] R119-002 — Reject aliased single-path self rename
- [x] R119-003 — Reject aliased self rename in dry-run preview
- [x] R119-004 — Reject same normalized path across repeated separators
- [x] R119-005 — Reject same normalized path across dot path segments
- [x] R119-006 — Reject alias duplicates during write batch preflight
- [x] R119-007 — Reject alias duplicates during read-only write preview
- [x] R119-008 — Reject same source within batch renames under aliases
- [x] R119-009 — Reject reused destination within batch renames under aliases
- [x] R119-010 — Reject source-to-previous-destination alias
- [x] R119-011 — Reject destination-to-previous-source alias
- [x] R119-012 — Preserve budget counts after rejected aliases
- [x] R119-013 — Preserve protected-path and malformed-path denial
- [ ] R119-014 — Qualify canonical file-handle write guard on QEMU persistent desktop

### W2: Network ownership and UDP generations
- [x] R119-015 — Add atomic UDP owner-retirement group API
- [x] R119-016 — Reject zero PID in UDP group retirement
- [x] R119-017 — Reject zero generation in UDP group retirement
- [x] R119-018 — Reject duplicate process ID in UDP retirement batch
- [x] R119-019 — Reject stale PID generation in UDP retirement batch
- [x] R119-020 — Preserve earlier valid members on a later failed member
- [x] R119-021 — Preserve unrelated live UDP owner and local port
- [x] R119-022 — Count exactly the UDP grants removed
- [x] R119-023 — Preserve new owner after PID recycling
- [x] R119-024 — Verify empty batch is idempotent
- [x] R119-025 — Verify zero-capacity UDP registry safety
- [x] R119-026 — Test port release for multiple ports of one process
- [ ] R119-027 — Wire group retirement through real network socket table
- [ ] R119-028 — Qualify two ring3 processes and foreign-close denial in QEMU

### W3: Capabilities and authenticated process lifetime
- [x] R119-029 — Add atomic kernel capability-epoch retirement group API
- [x] R119-030 — Preflight every owner before revoking any token
- [x] R119-031 — Reject repeated PID in capability group
- [x] R119-032 — Reject zero PID in capability group
- [x] R119-033 — Reject zero or inactive generation in capability group
- [x] R119-034 — Reject stale second owner without partial retirement
- [x] R119-035 — Preserve unretired peer capability ticket
- [x] R119-036 — Deny retired owner tickets after success
- [x] R119-037 — Deny replayed retirement after process exit
- [x] R119-038 — Deny old ticket after PID restarts
- [x] R119-039 — Verify empty capability retirement batch
- [x] R119-040 — Verify zero-capacity registry behavior
- [ ] R119-041 — Wire group retirement to scheduler normal-exit and fault paths
- [ ] R119-042 — Qualify real SMP lock ordering of multi-owner capability teardown

### W4: Scheduler lifecycle events and receipt replay
- [x] R119-043 — Add atomic group enqueue of exact-incarnation exit events
- [x] R119-044 — Deduplicate same PID-generation exit within a batch
- [x] R119-045 — Deduplicate exit already queued before the batch
- [x] R119-046 — Preserve existing receipt sequence during enqueue
- [x] R119-047 — Roll back batch if a later exit PID is invalid
- [x] R119-048 — Roll back batch on queue capacity exhaustion
- [x] R119-049 — Roll back batch on unique sequence exhaustion
- [x] R119-050 — Reject stale receipt after next-generation notice
- [x] R119-051 — Preserve distinct generation exit receipts
- [x] R119-052 — Preserve FIFO order of peer notices
- [x] R119-053 — Allow empty batch without side effects
- [x] R119-054 — Handle zero-capacity event queue safely
- [ ] R119-055 — Wire group event path to live scheduler teardown
- [ ] R119-056 — Qualify cross-core IPI and IRQ scheduler exit delivery

### W5: Release checks and evidence
- [x] R119-057 — Record exact merged main parent for R119
- [x] R119-058 — Keep five logical workstreams in one source commit
- [x] R119-059 — Add filesystem canonical alias negative regressions
- [x] R119-060 — Add generation-safe UDP batch negative regressions
- [x] R119-061 — Add capability group retire negative regressions
- [x] R119-062 — Add atomic event receipt batch negative regressions
- [ ] R119-063 — Run exact-head full Rust unit tests
- [ ] R119-064 — Run optimized freestanding no_std build
- [ ] R119-065 — Run two-boot persistent graphical Files QEMU
- [ ] R119-066 — Run ring3 UDP ownership QEMU
- [ ] R119-067 — Run kernel ELF/ISO serial boot evidence
- [ ] R119-068 — Require independent human kernel/security reviewer
- [ ] R119-069 — Require all mandatory final-SHA CI checks to pass before merge
- [ ] R119-070 — Publish merge SHA and unverified physical-hardware gates

### Inherited R118 items: first 30 of 51
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

## Inherited R118 items retained outside 100-item primary ledger: remaining 21
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

## Verification and actual limits

- Source areas changed: `fs_write_guard.rs`, `udp_owner_epoch.rs`, `capability.rs`, `process_events.rs`.
- New tests authored: 17 focused Rust host tests (5 filesystem, 4 UDP, 4 capability, 4 process-event).
- Newly checked source-level items: 55/70; remaining new items: 15; inherited: 51 still require task-specific acceptance evidence. Source checkboxes are **not** overall kernel completion percentages.
- Exact-commit host CI, no_std release, QEMU two-boot graphical filesystem, ring3 cross-process UDP, kernel serial boot, and independent review must be checked before merge. All physical SMP, IRQ/IOMMU, trusted firmware measurement, actual disk power-cut and physical restoration work requires hardware validation.
- Do not merge this branch just because code compiles or an automated workflow labelled `review` passes; require the project's actual approvals and release policy.
