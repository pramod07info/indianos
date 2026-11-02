# IndianOS R117 — 100 numbered acceptance items

Base main SHA: `08c3e537da319d7d8ecb976957ef9421b53d554d` (R116 PR #748 merged).

Items 001–070 are 70 granular source-level behaviors implemented across five logical engineering workstreams; **not 70 unrelated production-qualified kernel features**. Items 071–100 remain unchecked until integration/QEMU/physical hardware/release verification. All checked source behaviors require exact-head GitHub CI.

## 70 source-level code acceptance behaviors

### W1 — Atomic filesystem process retirement
- [x] R117-001 — Preflight batch capacity before removing a file owner
- [x] R117-002 — Reject a retired owner before mutating a peer
- [x] R117-003 — Reject stale same-PID incarnation in a batch
- [x] R117-004 — Reject duplicate PID retirement requests
- [x] R117-005 — Reject PID outside the kernel file-table domain
- [x] R117-006 — Reject zero generation identities
- [x] R117-007 — Revoke two concurrently live process generations
- [x] R117-008 — Invalidate old PID1 mount aliases on retirement
- [x] R117-009 — Invalidate old PID2 mount aliases on retirement
- [x] R117-010 — Do not transfer old PID2 alias to a replacement
- [x] R117-011 — Preserve fresh PID2 rights after failed batch
- [x] R117-012 — Keep both owners live after an invalid batch
- [x] R117-013 — Accept empty authenticated retirement batch without mutation
- [x] R117-014 — Preserve PID2 owner after PID1 retirement

### W2 — Exact-generation delegated capability renewal
- [x] R117-015 — Renew an unexpired exact-generation lease
- [x] R117-016 — Extend expiration without modifying granted rights
- [x] R117-017 — Reject a caller with a foreign PID
- [x] R117-018 — Reject a stale resource identifier
- [x] R117-019 — Reject a stale generation
- [x] R117-020 — Reject a stale capability epoch
- [x] R117-021 — Reject unauthorized rights escalation
- [x] R117-022 — Reject zero owner PID
- [x] R117-023 — Reject zero generation
- [x] R117-024 — Reject zero required rights
- [x] R117-025 — Never revive an already expired lease
- [x] R117-026 — Reject an equal or shortened expiry
- [x] R117-027 — Reject renewal of an absent or revoked lease
- [x] R117-028 — Keep a peer lease unchanged during one-owner renewal

### W3 — Lease-checked network transmission
- [x] R117-029 — Sample trusted clock before route selection
- [x] R117-030 — Require a valid time-scoped initial route receipt
- [x] R117-031 — Sample trusted clock again at dispatch
- [x] R117-032 — Reject a backwards-moving clock
- [x] R117-033 — Reject receipt expiry between clock samples
- [x] R117-034 — Do not write a packet after lease-window expiry
- [x] R117-035 — Permit explicit retry with a newly current backup receipt
- [x] R117-036 — Require a matching kernel route owner identity
- [x] R117-037 — Reject recycled routing principal generation
- [x] R117-038 — Reject route with no healthy candidate
- [x] R117-039 — Reject an expired backup path
- [x] R117-040 — Preserve NIC send count after a denied send
- [x] R117-041 — Deliver packets with a stable primary lease
- [x] R117-042 — Deliver packets using an authorized static backup

### W4 — Sealed-floor-guarded boot recovery
- [x] R117-043 — Observe independently supplied sealed floor before mutation
- [x] R117-044 — Verify trusted measurement before writing boot epoch
- [x] R117-045 — Advance one valid epoch with correct existing seal
- [x] R117-046 — Reject on-disk epoch below sealed minimum
- [x] R117-047 — Reject journal generation below sealed minimum
- [x] R117-048 — Reject boot slot different from sealed slot
- [x] R117-049 — Reject signed slot-version rollback
- [x] R117-050 — Reject mismatched expected boot digest
- [x] R117-051 — Reject claimed prior epoch inconsistent with disk
- [x] R117-052 — Reject alternate selected slot at advancement
- [x] R117-053 — Reject requested journal generation regression
- [x] R117-054 — Reject absent trusted measurement
- [x] R117-055 — Preserve media contents on failed admission
- [x] R117-056 — Use guarded advancement in disposable-disk QEMU boot path

### W5 — Exact-head lifecycle event acknowledgement
- [x] R117-057 — Reject acknowledgement of an empty queue
- [x] R117-058 — Accept matching lifecycle event head
- [x] R117-059 — Require nonzero expected process ID
- [x] R117-060 — Reject mismatched process generation
- [x] R117-061 — Reject different process ID
- [x] R117-062 — Reject different event kind
- [x] R117-063 — Reject mismatched message payload
- [x] R117-064 — Keep FIFO head unchanged after bad acknowledgement
- [x] R117-065 — Preserve successor PID-incarnation event
- [x] R117-066 — Preserve peer lifecycle message after stale receipt
- [x] R117-067 — Preserve event order across wrapped queue
- [x] R117-068 — Make repeated acknowledgement idempotent
- [x] R117-069 — Keep zero-capacity event queue fail-closed
- [x] R117-070 — Wire authenticated PID1 consumer to exact-head acknowledgement

## 30 outstanding integration, hardware and release gates

- [ ] R117-071 — Inherited R116-071 — Inherited R115-074 — Reject concurrent mount replacement during path lookup  
- [ ] R117-072 — Inherited R116-072 — Inherited R115-075 — Add adversarial ring3 namespace pointer QEMU boot test
- [ ] R117-073 — Inherited R116-073 — Inherited R115-078 — Enforce port-generation checks at network ingress IRQ
- [ ] R117-074 — Inherited R116-074 — Inherited R115-080 — Prove two guest processes cannot release peer ports in QEMU
- [ ] R117-075 — Inherited R116-075 — Inherited R115-083 — Exercise real ring3 delegated-handle revocation
- [ ] R117-076 — Inherited R116-076 — Inherited R115-084 — Audit lock ordering of file/network lease teardown
- [ ] R117-077 — Inherited R116-077 — Inherited R115-085 — Prove no scheduler teardown deadlock in SMP configuration
- [ ] R117-078 — Inherited R116-078 — Inherited R115-086 — Use trusted boot observer in production PID1 boot path
- [ ] R117-079 — Inherited R116-079 — Inherited R115-087 — Provision immutable boot measurement root in bootloader
- [ ] R117-080 — Inherited R116-080 — Inherited R115-088 — Verify boot journal under actual unsafe power cut
- [ ] R117-081 — Inherited R116-082 — Inherited R115-090 — Qualify physical boot-sector restoration on hardware
- [ ] R117-082 — Inherited R116-083 — Inherited R115-092 — Read a kernel monotonic timestamp at transmit and retry
- [ ] R117-083 — Inherited R116-085 — Inherited R115-095 — Qualify multiple physical NICs with IOMMU isolation
- [ ] R117-084 — Inherited R116-086 — Inherited R115-096 — Complete required no_std release kernel build
- [ ] R117-085 — Inherited R116-087 — Inherited R115-097 — Pass all host Rust tests on the exact integration commit
- [ ] R117-086 — Inherited R116-088 — Inherited R115-098 — Pass persistent graphical desktop and VFS QEMU workflow
- [ ] R117-087 — Inherited R116-090 — Inherited R115-100 — Complete independent review and merge only with green required CI
- [ ] R117-088 — Inherited R116-091 — Run ring3 QEMU two-process foreign-port close denial end to end
- [ ] R117-089 — Inherited R116-092 — Run pointer-fuzz ring3 alias open with guard-page crossing
- [ ] R117-090 — Inherited R116-093 — Implement physical multicore scheduler lock ordering and IPI safety
- [ ] R117-091 — Inherited R116-094 — Provide immutable production bootloader measurement root
- [ ] R117-092 — Inherited R116-096 — Verify actual physical power-loss recovery and boot-sector repair
- [ ] R117-093 — Inherited R116-097 — Qualify multiple physical NICs with IOMMU DMA isolation
- [ ] R117-094 — Inherited R116-098 — Require exact commit host Rust/no_std, QEMU network, graphical VFS and boot workflows
- [ ] R117-095 — Inherited R116-099 — Complete independent review and branch protection before merge
- [ ] R117-096 — Inherited R116-100 — Publish verified post-merge main SHA and record release evidence
- [ ] R117-097 — Integrate a hardware-calibrated kernel monotonic clock into live NIC dispatch
- [ ] R117-098 — Run real two-guest ring3 foreign-port-close and lease-revocation QEMU acceptance
- [ ] R117-099 — Complete a final exact-head persistent graphical Files reboot and release matrix
- [ ] R117-100 — Publish independent review, protected merge SHA and hardware-vs-QEMU evidence

## Actual production integration

- Filesystem (`fs_handles.rs`): serialized fail-atomic multi-process retirement with exact PID-generation preflight and alias invalidation.
- Capability (`agent_03_cap_epoch.rs`): exact-grant lease renewal that cannot increase rights, alter identity, shorten a deadline or revive expiry.
- Network (`network.rs`): two clock samples and time-scoped route receipt revalidation before the actual `EthernetDevice::send` method, with network mock tests. The existing QEMU demo still uses a *synthetic* clock; a real monotonic kernel clock gate remains pending.
- Boot (`r114_boot_epoch.rs`, `kernel_main.rs`): authenticated sealed-floor preflight before epoch writes; invoked in the disposable QEMU boot profile. This is NOT a provisioned physical root of trust.
- Runtime (`process_events.rs`, `user_runtime.rs`): exact-head event acknowledgement by the already authenticated PID1 consumer, without dequeueing a successor or peer.

## Previous batch reconciliation

R116-081, R116-084, R116-089 and R116-095 previously passed the matching QEMU tests on the prior PR head; they are not repeated as open tasks here. QEMU is not evidence of physical power-cut, hardware IOMMU, production TPM or SMP qualification.

Five *logical* workstreams; no five independent autonomous engineering agents were actually launched. One integrated source commit, one PR, no merge until all required exact-head CI checks and independent reviews pass.

## R117 real-path integration addition

The live single-BSP scheduler now invokes the exact-generation file retirement batch for its actual process teardown; the existing QEMU virtio-net packet send path now uses the double-sampled trusted-clock contract, with a **synthetic QEMU test clock only**. This does not qualify physical monotonic IRQ timing or SMP.

## R117 multi-commit integration continuation

Three focused implementation commits are permitted for this continuation at the user's explicit request; the previous one-commit rule is superseded **for this PR**. Current changes include (1) ring-3 authenticated socket-close helper wired to live syscall 36 with stale/foreign owner adversarial tests, (2) namespace + selected-volume revision guard before and after the actual partition mount probe, and (3) mandatory targeted `r117_` host tests, optimized no_std build and persistent graphical Files QEMU workflow on modified source paths. R117-071, 074, 084, 086, 088, 094 and 099 remain pending **full exact-head CI/acceptance** and SMP/hardware qualifications. Real multi-CPU concurrency is NOT proven by the single-BSP guard; COW disk simulation is NOT hardware power-cut evidence. Never mark these gates green from text or a successful cargo check alone.

## R117 pointer-fuzz and release-gate reconciliation

Latest previously green PR SHA `63a7f1b5ead15175c7a8cc1128462c64f52e3963` had **10/10 GitHub checks success**, including `Build ELF + ISO and verify serial boot`, `durable-files-desktop`, `combined-regression`, `runtime-qemu`, `network`, `qemu-epoch`, and adversarial security. That provides prior-commit evidence for R117-084 (optimized freestanding no_std), R117-085 (host Rust), R117-086 (persistent Files GUI QEMU), and R117-094 (CI matrix). R117-099's real Files reboot test also passed, but final release signoff is distinct. Reverify all relevant checks on the **new final head** before changing checkboxes or merging.

R117-072 and R117-089 now additionally have real `user_image` page-table based tests (`r117_guest_*`) for unmapped second page, read-only second page, invalid pointer overflow, and guard boundary denial; they are not equivalent to the specifically requested *adversarial ring-3 QEMU executable* acceptance. The graphical QEMU workflow now triggers when `user_image.rs` changes and runs these exact tests. Full guest-QEMU negative cases remain open.

The user's request is for all 30 complete. Hardware-dependent R117-077/079/080/081/083/090/091/092/093/097 and full hardware IOMMU/power-loss paths cannot be responsibly checked without implementation plus physical lab evidence. The source tests only support the stated **single-BSP kernel** and **emulated** hardware scope.
