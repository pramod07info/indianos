# IndianOS R116 — Five integrated kernel workstreams / 100-item evidence ledger

**Base:** `36739905f093916cbaea27971e4f561ed9a8fa31` (merged R115 PR #746).
**Commit policy:** exactly one consolidated source commit and one PR; five logical workstreams, not five independent running workstations.
**Interpretation:** items 001–070 are granular implementable code-level checks across real Rust interfaces and adversarial tests; **they are not 70 independent production features**. They are staged for exact-commit CI validation. Items 071–100 are explicitly unverified live integration, QEMU, hardware, SMP and release gates.

## 70 new source-level acceptance checks

### W1 — Authenticated selected-volume VFS
- [x] R116-001 — Bind alias operations to the exact selected physical partition receipt
- [x] R116-002 — Reject unselected-volume aliases before disk reads
- [x] R116-003 — Authenticate kernel-issued PID and incarnation for alias open
- [x] R116-004 — Reject stale process incarnation after PID reuse
- [x] R116-005 — Reject alias operations on quarantined physical volume
- [x] R116-006 — Allow bounded read open for current guest root
- [x] R116-007 — Reject guest create or mutation under read-only shared root
- [x] R116-008 — Reject guest write handle request through selected-volume alias
- [x] R116-009 — Reject sibling-prefix namespace escape
- [x] R116-010 — Reject parent-directory traversal without probing the disk
- [x] R116-011 — Reject relative path escape without probing the disk
- [x] R116-012 — Reject exact directory-root as file open
- [x] R116-013 — Preflight all denied requests without storage reads or writes
- [x] R116-014 — Keep valid aliases mount-revision pinned across verified volume selection

### W2 — Scheduler-bound delegated capabilities
- [x] R116-015 — Introduce kernel-owned bounded directory lease table
- [x] R116-016 — Mint delegated authority only after trusted PID1 and PID2 preparation
- [x] R116-017 — Require exact current FileHandles incarnation for both issuer and recipient
- [x] R116-018 — Fix directory resource to kernel-selected shared tree
- [x] R116-019 — Fix rights to read-only directory inspection
- [x] R116-020 — Attenuate rights at the trusted PID1 issuer
- [x] R116-021 — Reject wrong recipient PID
- [x] R116-022 — Reject recycled recipient generation
- [x] R116-023 — Reject missing PID2 lease at actual ring3 directory syscall
- [x] R116-024 — Reject stale epoch in lease authorization
- [x] R116-025 — Reject zero generation in delegated recipient grant
- [x] R116-026 — Reject escalation or foreign resource delegation
- [x] R116-027 — Revoke exact leases during scheduler exit, fault and rollback
- [x] R116-028 — Preserve live peer resources when stale teardown attempts revocation

### W3 — Registered ring-3 UDP data plane
- [x] R116-029 — Require kernel registry authorization for real ring3 UDP send
- [x] R116-030 — Require kernel registry authorization for real ring3 UDP receive
- [x] R116-031 — Reject stale sender generation before local enqueue
- [x] R116-032 — Reject stale receiver generation before queue consumption
- [x] R116-033 — Reject cross-owner source-port sends
- [x] R116-034 — Reject reads from other process's bound port
- [x] R116-035 — Fail closed with empty owner registry
- [x] R116-036 — Reject dual-table registry/socket inconsistency
- [x] R116-037 — Keep queued message untouched after unauthorized receive
- [x] R116-038 — Keep caller output untouched on unauthorized receive
- [x] R116-039 — Preserve peer datagrams across one owner's retirement
- [x] R116-040 — Disallow a retired process from injecting to live peers
- [x] R116-041 — Allow authorized reincarnation with clean socket queues
- [x] R116-042 — Retain previous registered bind, close and ingress enforcement

### W4 — Sealed boot-floor recovery policy
- [x] R116-043 — Define monotonic minimum accepted boot epoch
- [x] R116-044 — Require minimum authenticated journal generation
- [x] R116-045 — Pin selected signed boot slot
- [x] R116-046 — Require minimum signed slot version
- [x] R116-047 — Pin expected boot measurement digest
- [x] R116-048 — Reject zero minimum boot epoch
- [x] R116-049 — Reject zero minimum journal generation
- [x] R116-050 — Reject zero minimum slot version
- [x] R116-051 — Reject invalid slot selection
- [x] R116-052 — Reject zero expected measurement root
- [x] R116-053 — Refuse wrong trusted measurement source
- [x] R116-054 — Refuse forged journal predecessor digest
- [x] R116-055 — Refuse divergent same-epoch journal copies
- [x] R116-056 — Exercise floor observation in real QEMU epoch test path without pretending fixture is TPM

### W5 — Process-incarnation event integrity
- [x] R116-057 — Deduplicate exact PID-generation exit notices
- [x] R116-058 — Reject zero PID in lifecycle exit notices
- [x] R116-059 — Reject zero generation in lifecycle exit notices
- [x] R116-060 — Maintain bounded zero-capacity fail-closed behavior
- [x] R116-061 — Reject full queue without corrupting existing events
- [x] R116-062 — Re-enqueue only after previous generation message is consumed
- [x] R116-063 — Remove exact matching exit generation only
- [x] R116-064 — Retain successor generation's exit notice
- [x] R116-065 — Retain same-PID ordinary message payloads
- [x] R116-066 — Preserve peer PID events
- [x] R116-067 — Preserve FIFO order after partial deletion
- [x] R116-068 — Handle wrapped-ring duplicate detection
- [x] R116-069 — Make exact-generation clear idempotent
- [x] R116-070 — Wire scheduler authority retirement to once-only exit events

## 30 remaining gates (including all inherited R115 open items)

- [ ] R116-071 — Inherited R115-074 — Reject concurrent mount replacement during path lookup  
- [ ] R116-072 — Inherited R115-075 — Add adversarial ring3 namespace pointer QEMU boot test
- [ ] R116-073 — Inherited R115-078 — Enforce port-generation checks at network ingress IRQ
- [ ] R116-074 — Inherited R115-080 — Prove two guest processes cannot release peer ports in QEMU
- [ ] R116-075 — Inherited R115-083 — Exercise real ring3 delegated-handle revocation
- [ ] R116-076 — Inherited R115-084 — Audit lock ordering of file/network lease teardown
- [ ] R116-077 — Inherited R115-085 — Prove no scheduler teardown deadlock in SMP configuration
- [ ] R116-078 — Inherited R115-086 — Use trusted boot observer in production PID1 boot path
- [ ] R116-079 — Inherited R115-087 — Provision immutable boot measurement root in bootloader
- [ ] R116-080 — Inherited R115-088 — Verify boot journal under actual unsafe power cut
- [ ] R116-081 — Inherited R115-089 — Run multi-boot OVMF persistent-disk failure matrix
  Acceptance implemented in R116's actual QEMU harness; CHECK REMAINS OPEN until the final commit's exact-head QEMU workflow verifies all serial markers and negative controls.
- [ ] R116-082 — Inherited R115-090 — Qualify physical boot-sector restoration on hardware
- [ ] R116-083 — Inherited R115-092 — Read a kernel monotonic timestamp at transmit and retry
- [ ] R116-084 — Inherited R115-094 — Test QEMU UDP resend across timed route invalidation  
  Acceptance implemented in R116's actual QEMU harness; CHECK REMAINS OPEN until the final commit's exact-head QEMU workflow verifies all serial markers and negative controls.
- [ ] R116-085 — Inherited R115-095 — Qualify multiple physical NICs with IOMMU isolation
- [ ] R116-086 — Inherited R115-096 — Complete required no_std release kernel build
- [ ] R116-087 — Inherited R115-097 — Pass all host Rust tests on the exact integration commit
- [ ] R116-088 — Inherited R115-098 — Pass persistent graphical desktop and VFS QEMU workflow
- [ ] R116-089 — Inherited R115-099 — Pass QEMU boot epoch and virtio NIC workflows
- [ ] R116-090 — Inherited R115-100 — Complete independent review and merge only with green required CI
- [ ] R116-091 — Run ring3 QEMU two-process foreign-port close denial end to end
- [ ] R116-092 — Run pointer-fuzz ring3 alias open with guard-page crossing
- [ ] R116-093 — Implement physical multicore scheduler lock ordering and IPI safety
- [ ] R116-094 — Provide immutable production bootloader measurement root
- [ ] R116-095 — Run OVMF persistent disk multi-boot fault-injection qualification
  Acceptance implemented in R116's actual QEMU harness; CHECK REMAINS OPEN until the final commit's exact-head QEMU workflow verifies all serial markers and negative controls.
- [ ] R116-096 — Verify actual physical power-loss recovery and boot-sector repair
- [ ] R116-097 — Qualify multiple physical NICs with IOMMU DMA isolation
- [ ] R116-098 — Require exact commit host Rust/no_std, QEMU network, graphical VFS and boot workflows
- [ ] R116-099 — Complete independent review and branch protection before merge
- [ ] R116-100 — Publish verified post-merge main SHA and record release evidence

## Changes and actual verification policy

- W1: `rust/kernel_core/src/fs_handles.rs`: selected-volume alias checked against exact volume receipt, current namespace and canonical relative path before disk probing; fixture tests.
- W2: `rust/kernel_core/src/agent_03_cap_epoch.rs` + `user_runtime.rs`: scheduler-issued PID2 shared-directory read lease, checked on real ring3 directory enumeration and retired on exact owner exit/fault/rollback.
- W3: `rust/kernel_core/src/network.rs` + `user_runtime.rs`: registered ring3 data-plane send and receive, with negative-control peer, stale generation and queue tests.
- W4: `rust/kernel_core/src/r114_boot_epoch.rs` + `kernel_main.rs`: independently sealed rollback floor policy exercised in QEMU disposable test. **Production boot trust is still unimplemented**.
- W5: `rust/kernel_core/src/process_events.rs` + `user_runtime.rs`: once-only exact-generation exit notices and safe queue semantics.

CI status and merge are checked against the final commit only. This ledger is not a substitute for compiled host tests, QEMU boot, production hardware or a real physical power cut.

## Previous batch reconciliation

R115-081 and R115-082 source paths are integrated in this batch. The original remaining R115 items are carried forward verbatim above; no hardware/SMP/TPM claim is upgraded on the basis of a unit test.


## R116 pending-gate QEMU continuation

Real QEMU acceptance code added for R116-081 (multi-boot OVMF corrupt-journal matrix), R116-084 (actual guest UDP retransmit following expired primary route receipt), and R116-095 (multi-boot fault-injection qualification on disposable emulated media). They remain **unchecked** until exact-head CI passes. The OVMF tests simulate checksum/chain failure but do not simulate unexpected physical power loss. Independent physical-IOMMU and SMP gates remain open.
