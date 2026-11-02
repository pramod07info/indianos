# IndianOS R115 — 100-item kernel-first integrity batch

Source base: `e1a00ae4beb37dd5fbeed79005a90733fc8e3d44`

**Accounting:** 70 narrowly-scoped **original source-level acceptance checks implemented in Rust and regressions**, plus 6 integration tasks now backed by new source modifications (pending exact-head CI) across five logical workstreams, plus 30 explicitly **unchecked** integration/hardware/CI release gates. The 70 checkboxes are NOT 70 independently shipped OS features, NOT five autonomously executed agents and NOT verified as passing before exact-head CI.

Each workstream changes an existing kernel-core Rust module. Tests are host-side; new APIs do not automatically bind themselves to production syscalls, bootloader or physical NIC devices.

## W1 — Canonical VFS subtree-relative lookup
- [x] R115-001 — Add allocation-free relative path conversion
- [x] R115-002 — Pre-normalize candidate path
- [x] R115-003 — Pre-normalize namespace root
- [x] R115-004 — Reject sibling-prefix lookalikes
- [x] R115-005 — Deny path outside namespace
- [x] R115-006 — Return zero length for exact namespace root
- [x] R115-007 — Handle namespace root slash
- [x] R115-008 — Reject parent traversal in candidate
- [x] R115-009 — Reject parent traversal in root
- [x] R115-010 — Reject relative candidate and root
- [x] R115-011 — Reject embedded NUL on either operand
- [x] R115-012 — Reject short outputs before writing
- [x] R115-013 — Preserve unrelated output bytes
- [x] R115-014 — Honor UTF-8 byte and component limits

## W2 — Transactional UDP owner retirement
- [x] R115-015 — Add exact-owner multi-port revoke
- [x] R115-016 — Stage all removals in local registry
- [x] R115-017 — Withdraw multiple owned ports
- [x] R115-018 — Withdraw grants for multiple exact owners
- [x] R115-019 — Preserve unrelated owner ports
- [x] R115-020 — Deny recycled PID generations
- [x] R115-021 — Deny foreign process claims
- [x] R115-022 — Reject zero PID
- [x] R115-023 — Reject zero PID-generation
- [x] R115-024 — Reject port zero
- [x] R115-025 — Detect duplicate batch ports
- [x] R115-026 — Roll back earlier withdrawals on missing port
- [x] R115-027 — Support empty no-op batch
- [x] R115-028 — Fail safely in zero-capacity registry

## W3 — Transactional timed capability retirement
- [x] R115-029 — Add exact-incarnation lease batch revoke
- [x] R115-030 — Stage lease removals before publishing
- [x] R115-031 — Allow multiple process incarnations in one operation
- [x] R115-032 — Preserve unrelated valid peer lease
- [x] R115-033 — Reject stale PID generation
- [x] R115-034 — Reject missing resource lease
- [x] R115-035 — Reject nonexistent process lease
- [x] R115-036 — Reject zero PID
- [x] R115-037 — Reject zero generation
- [x] R115-038 — Detect duplicate exact identity
- [x] R115-039 — Roll back earlier lease removals on failure
- [x] R115-040 — Support empty batch
- [x] R115-041 — Preserve zero-capacity table
- [x] R115-042 — Keep revoked leases inaccessible

## W4 — Authenticated boot epoch observation
- [x] R115-043 — Expose read-only verified journal observation
- [x] R115-044 — Require trusted measurement capability
- [x] R115-045 — Reject zero trusted digest
- [x] R115-046 — Read and validate both journal sectors
- [x] R115-047 — Check journal predecessor SHA256 chain
- [x] R115-048 — Require digest match for latest epoch
- [x] R115-049 — Expose verified epoch number
- [x] R115-050 — Expose journal generation
- [x] R115-051 — Expose selected boot slot
- [x] R115-052 — Expose signed slot version
- [x] R115-053 — Reject divergent same-epoch copies
- [x] R115-054 — Reject forged predecessor digest
- [x] R115-055 — Avoid writes during valid observation
- [x] R115-056 — Avoid writes during failed observation

## W5 — Time-aware route receipts
- [x] R115-057 — Expose deadline-aware route receipt
- [x] R115-058 — Revalidate receipt at monotonic time
- [x] R115-059 — Select lower-metric live route
- [x] R115-060 — Return static backup after DHCP expiry
- [x] R115-061 — Invalidate exactly at lease deadline
- [x] R115-062 — Avoid route-table mutation during read
- [x] R115-063 — Reject stale receipt after link reset
- [x] R115-064 — Select peer during quarantine
- [x] R115-065 — Invalidate pre-reset route epoch
- [x] R115-066 — Accept restored link under new epoch
- [x] R115-067 — Permit correctly renewed active lease
- [x] R115-068 — Reject receipt at renewed deadline
- [x] R115-069 — Return no receipt with empty route table
- [x] R115-070 — Invalidate receipt after exact link removal

## Pending integration, hardware and release checks
- [x] R115-071 — Connect relative VFS API to the live authenticated PID2 open syscall  
  Evidence: Integrated: live ring3 PID2 Files OPEN now mints and uses exact current-root alias before ATA probe; blocked denial tests
- [x] R115-072 — Authenticate mount root directly from scheduler authority  
  Evidence: Integrated: kernel process namespace and generation are minted into root alias; user descriptor cannot supply it
- [x] R115-073 — Validate namespace aliases during filesystem remount  
  Evidence: selected physical volume remount and healthy quarantine recovery rotate a monotonic mount revision embedded in kernel-issued aliases; stale aliases fail before disk I/O. Host tests exercise real selected-volume mount and recovery.
  Status: Partial: root narrowing invalidates aliases; physical remount alias version synchronization still pending
- [ ] R115-074 — Reject concurrent mount replacement during path lookup  
  Status: Partial: explicit selected-volume remount rechecks namespace receipt before dispatch; SMP concurrency not qualified
- [ ] R115-075 — Add adversarial ring3 namespace pointer QEMU boot test
- [x] R115-076 — Bind UDP batch unbind to scheduler crash teardown
  Evidence: real ring-3 syscall 36 binds to kernel port registry, close withdraws both registered port and live endpoint, and scheduler exit/fault rollback retires exact owner epoch while keeping other PIDs.
- [x] R115-077 — Route UDP grant revocation through actual socket table  
  Evidence: Integrated: batch UDP policy revocation now coordinates live endpoint and queue closes under serialized kernel caller
- [ ] R115-078 — Enforce port-generation checks at network ingress IRQ
  Partial: the QEMU NIC's real frame receive path now calls a registry-authenticated ingress function before checksum parsing and enqueue. Physical IRQ/SMP receive dispatch has not been verified.
- [x] R115-079 — Reject packets queued across PID reincarnation  
  Evidence: Integrated: closing ports discards queued messages; PID-generation-reuse acceptance exercised in host tests
- [ ] R115-080 — Prove two guest processes cannot release peer ports in QEMU
- [x] R115-081 — Wire lease batch revoke to scheduler PCB lifecycle
  R116 source integration: Scheduler revoke_task_authority now revokes generation-specific LeaseTable entries before retiring FileHandles for every exit/fault/rollback. Exact-head CI required; this is not an SMP qualification.
- [x] R115-082 — Bind lease table identity to real kernel process capability
  R116 source integration: PID2 shared-directory lease is minted by the trusted scheduler only after both FileHandles process generations are live, and checked at actual ring3 syscall 34. Exact-head CI required; this is not an SMP qualification.
- [ ] R115-083 — Exercise real ring3 delegated-handle revocation
- [ ] R115-084 — Audit lock ordering of file/network lease teardown
- [ ] R115-085 — Prove no scheduler teardown deadlock in SMP configuration
- [ ] R115-086 — Use trusted boot observer in production PID1 boot path
- [ ] R115-087 — Provision immutable boot measurement root in bootloader
- [ ] R115-088 — Verify boot journal under actual unsafe power cut
- [ ] R115-089 — Run multi-boot OVMF persistent-disk failure matrix
- [ ] R115-090 — Qualify physical boot-sector restoration on hardware
- [x] R115-091 — Connect time-scoped receipt to actual NIC transmit  
  Evidence: Integrated in QEMU-specific live EthernetDevice send path: deadline-aware FailoverRoutes receipts
- [ ] R115-092 — Read a kernel monotonic timestamp at transmit and retry
- [x] R115-093 — Reject pending send after DHCP lease expiry  
  Evidence: Integrated in QEMU-specific send path: lease expiry forces backup or denies transmission
- [ ] R115-094 — Test QEMU UDP resend across timed route invalidation  
  Status: Partial: QEMU peer existing failover re-transmit uses leased route receipts; real expiration-fault injection still pending
- [ ] R115-095 — Qualify multiple physical NICs with IOMMU isolation
- [ ] R115-096 — Complete required no_std release kernel build
- [ ] R115-097 — Pass all host Rust tests on the exact integration commit
- [ ] R115-098 — Pass persistent graphical desktop and VFS QEMU workflow
- [ ] R115-099 — Pass QEMU boot epoch and virtio NIC workflows
- [ ] R115-100 — Complete independent review and merge only with green required CI

## Validation policy
- [ ] Exact-head `cargo test` on GitHub Actions
- [ ] Exact-head Rust `no_std` kernel build
- [ ] Exact-head QEMU/OVMF desktop persistence and networking evidence
- [ ] Mandatory reviews/branch protection and only then merge

## Actual change summary
- `rust/kernel_core/src/vfs_path.rs`: R115 checked relative VFS
- `rust/kernel_core/src/udp_owner_epoch.rs`: R115 atomic port release
- `rust/kernel_core/src/agent_03_cap_epoch.rs`: R115 lease revocation
- `rust/kernel_core/src/r114_boot_epoch.rs`: R115 trusted boot observation
- `rust/kernel_core/src/agent_06_ipv4_route.rs`: R115 time-scoped route receipts

Do not turn any hardware item green based solely on QEMU, host tests, or written checklist entries.

## R115 integration continuation

The original 70-item ledger is preserved. Eight additional integration boxes are checked only as code-level changes, not as physical hardware qualifications. The live PID2 Files open path now invokes kernel-minted namespace aliases; a new batch UDP withdrawal coordinates the actual queue and registry; QEMU network demo sends via time-scoped route receipts; the QEMU boot epoch success requires verified read-only observation. Existing negative-control tests are retained. Exact final head CI still required.

## R115 kernel lifecycle continuation

- R115-076 code integration: `user_runtime.rs` now maintains a kernel-only UDP grant registry beside the actual ring-3 socket queue. The real syscall 36 bind, close and scheduler termination use the registered endpoint API. The paired tables reset on session rollback; stale generations are rejected. The new `network.rs` host regressions exercise queued-message retirement, replacement identity denial and peer preservation.
- This is the **single-BSP** runtime, not an SMP/IRQ lock solution. The `R115-085` and physical NIC checks remain unchecked.
- No production bootloader TPM/root-of-trust claims or hardware qualification claims are made.

## R115 receive-side hardening

The existing QEMU virtio NIC demo now checks the same live UDP PID-generation registry used by ring-3 bind, close and scheduler teardown before feeding any Ethernet frame into the bounded socket parser. Two adversarial regression tests deny stale/absent registry grants and preserve a peer after exact-owner retirement. **This is QEMU NIC path integration, not physical IRQ qualification**; R115-078 stays unchecked until IRQ work is proven.

## R115 selected-volume alias invalidation

`fs_handles.rs` now binds `MountAlias` to the selected physical partition receipt and a monotonic mount revision. First verified selection and explicit healthy recovery rotate this revision; quarantine denies all aliases. Stale aliases fail before disk access even if the original path spelling remains valid. This is source/fixture verification; it is not physical hotplug or hardware media-swap qualification.

## R116 continuity

R115-081 and R115-082 now have production-path single-BSP source wiring in R116, described above. Remaining R115 unchecked items are preserved for testing, QEMU acceptance or physical hardware qualification. R116 is a distinct one-commit integration PR and will not be merged until mandatory exact-head checks pass.
