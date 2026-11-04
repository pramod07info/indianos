# IndianOS R123 — 70 development obligations + 30 inherited gates

Base main: `3e10ba7096819a1e204c9d4b767d802dc12f3164` (merged PR #760). Work branch: `feature/r123-kernel-safety-release-ledger-20261104`.

**Scope honesty:** Five workstreams are engineering groups, not five independently running AI agents. The R122 historical baseline was 13/70 source/test authored, with 87/100 acceptance obligations not newly qualified. A merged PR is not evidence of hardware or release completion.

**R123 source authored in this integration commit (NOT CI-accepted yet):**
- SMP: single-BSP read-only outstanding receipt diagnostic and negative lifecycle regression.
- NIC: ring-instance-scoped in-flight RX admission count and regression.
- UDP: transactional *policy registry* generation replacement and rollback tests; not wired to hardware IRQ or live socket retirement.
- Filesystem: namespace-bound canonical child join with atomic output and negative tests; does not authorize I/O or synchronize remount.
- Boot: reject noncanonical or unaligned Limine HHDM offsets in Rust staging, with tests; does not attest firmware.
- Release: require admin enforcement, mandatory review conversation resolution, and disabled force-push/deletions in branch-protection evidence, with regressions.

No real APIC IPI, production TLB shootdown, DMA masking, TPM key provisioning, physical unsafe power cut, human review, or protected-main setting was implemented by this batch.

## 70 R123 development tasks — acceptance pending (5 groups × 14)

### Workstream 1 — SMP / CPU lifecycle
- [ ] R123-001 — Implement real APIC IPI send path
- [ ] R123-002 — Bind IPI receipt to physical local-APIC identity
- [ ] R123-003 — Deliver AP-side IPI dispatch callback
- [ ] R123-004 — Enforce IPI completion memory barriers
- [ ] R123-005 — Implement TLB shootdown request queue
- [ ] R123-006 — Require all target AP acknowledgements before frame reuse
- [ ] R123-007 — Expose fail-closed BSP outstanding receipt diagnostics
- [ ] R123-008 — Join scheduler process exit to AP acknowledgement
- [ ] R123-009 — Exercise two-vCPU simultaneous process teardown
- [ ] R123-010 — Inject AP no-progress and aborted cleanup faults
- [ ] R123-011 — Prove scheduler retirement lock ordering
- [ ] R123-012 — Audit cross-BSP shared table read access
- [ ] R123-013 — Run real multi-core hardware memory ordering stress
- [ ] R123-014 — Qualify physical SMP scheduler lifecycle

### Workstream 2 — NIC / UDP ingress security
- [ ] R123-015 — Stop hardware RX DMA before ring retirement
- [ ] R123-016 — Mask MSI-X at NIC queue shutdown
- [ ] R123-017 — Drain real NIC descriptor completion queue
- [ ] R123-018 — Bind IRQ callback to RX ring generation
- [ ] R123-019 — Audit IRQ-to-UDP PID revision revalidation
- [ ] R123-020 — Integrate exact socket owner publication fencing
- [ ] R123-021 — Implement atomic UDP grant generation replacement in policy registry
- [ ] R123-022 — Reconcile UDP policy replacement with live socket table
- [ ] R123-023 — Run real QEMU UDP rollback under PID recycling
- [ ] R123-024 — Reject stale descriptor completion after rebind
- [ ] R123-025 — Expose ring-scoped in-flight software admission count
- [ ] R123-026 — Verify multi-NIC port and ring isolation
- [ ] R123-027 — Qualify IOMMU DMA domain revoke and rebind
- [ ] R123-028 — Qualify physical interrupt ingress ownership

### Workstream 3 — Filesystem / VFS / persistence
- [ ] R123-029 — Serialize mount revision lookup across actual BSP peers
- [ ] R123-030 — Reject remount during path-to-handle transition
- [ ] R123-031 — Bind selected volume to immutable device identity
- [ ] R123-032 — Revalidate owner root after every mount revision
- [ ] R123-033 — Implement full journal torn-write rollback
- [ ] R123-034 — Verify disk flush errors leave durable snapshots
- [ ] R123-035 — Exercise two-cold-boot graphical Files checksum
- [ ] R123-036 — Run cross-service retire-owner deadlock stress
- [ ] R123-037 — Implement namespace-root child join with atomic output
- [ ] R123-038 — Bind namespace result to current mounted-root authority
- [ ] R123-039 — Reject stale PID file handle after process recycling
- [ ] R123-040 — Verify canonical rename alias during remount
- [ ] R123-041 — Qualify SMP remount and path lookup hardware race
- [ ] R123-042 — Qualify unsafe physical power-loss recovery

### Workstream 4 — Boot / firmware / trust
- [ ] R123-043 — Validate bootloader-provided memory-map spans
- [ ] R123-044 — Pin immutable verified boot trust anchor
- [ ] R123-045 — Authenticate kernel digest against trusted boot key
- [ ] R123-046 — Record measured kernel boot epoch before mount
- [ ] R123-047 — Reject boot epoch rollback at PID1 startup
- [ ] R123-048 — Validate user image memory map and executable limits
- [ ] R123-049 — Reject noncanonical/unaligned Limine HHDM base in staging tests
- [ ] R123-050 — Audit GDT IDT TSS bootstrap sequencing
- [ ] R123-051 — Hold interrupts masked before kernel lifecycle registration
- [ ] R123-052 — Authenticate native module execution policy
- [ ] R123-053 — Document and test offline key rotation
- [ ] R123-054 — Verify recovery image signature
- [ ] R123-055 — Qualify TPM firmware measured-boot chain
- [ ] R123-056 — Qualify physical boot-sector restoration

### Workstream 5 — CI / integration / protected release
- [ ] R123-057 — Run final-head Rust host test matrix
- [ ] R123-058 — Compile exact-head optimized no_std kernel
- [ ] R123-059 — Boot kernel ELF and ISO and inspect serial result
- [ ] R123-060 — Run two-vCPU AP cleanup and receipt QEMU test
- [ ] R123-061 — Run QEMU virtio UDP and DNS ingress
- [ ] R123-062 — Run ring3 multi-process recycle regression
- [ ] R123-063 — Run persistent graphical Files cold reboot
- [ ] R123-064 — Run journal rollback injection in QEMU
- [ ] R123-065 — Reject missing and skipped required status checks
- [ ] R123-066 — Reject human approval from outdated PR heads
- [ ] R123-067 — Fail closed on admin branch-protection bypass
- [ ] R123-068 — Reject force-push and branch deletion escape hatches
- [ ] R123-069 — Require resolved conversations and independent review
- [ ] R123-070 — Record immutable exact-head and post-merge evidence

## 30 prioritized inherited R122 acceptance gates — still pending

- [ ] R123-I01 — R121 carry-forward: R120-013 — cross-core lock-free access protection
- [ ] R123-I02 — R121 carry-forward: R120-026 — real interrupt ingress generation ownership
- [ ] R123-I03 — R121 carry-forward: R120-028 — physical multi-NIC IRQ and IOMMU qualification
- [ ] R123-I04 — R121 carry-forward: R120-042 — cross-core IPI lifecycle ordering
- [ ] R123-I05 — R121 carry-forward: R120-070 — independent review and protected merge evidence
- [ ] R123-I06 — R121 carry-forward: R118-001 — cross-BSP mount revision synchronization
- [ ] R123-I07 — R121 carry-forward: R118-003 — concurrent mount replacement stale revision denial
- [ ] R123-I08 — R121 carry-forward: R118-009 — deterministic deadlock-free file-owner retirement
- [ ] R123-I09 — R121 carry-forward: R118-013 — selected-volume token fuzz in QEMU
- [ ] R123-I10 — R121 carry-forward: R118-014 — physical SMP remount/lookup concurrency
- [ ] R123-I11 — R121 carry-forward: R118-027 — two-BSP scheduler revocation stress
- [ ] R123-I12 — R121 carry-forward: R118-028 — actual multicore teardown and IPI safety
- [ ] R123-I13 — R121 carry-forward: R118-040 — foreign port close denial guest acceptance
- [ ] R123-I14 — R121 carry-forward: R118-041 — calibrated kernel monotonic source
- [ ] R123-I15 — R121 carry-forward: R118-042 — physical NIC ingress IRQ qualification
- [ ] R123-I16 — R121 carry-forward: R118-054 — immutable production bootloader trust anchor
- [ ] R123-I17 — R121 carry-forward: R118-055 — physical destructive power-loss recovery
- [ ] R123-I18 — R121 carry-forward: R118-056 — hardware boot-sector restoration
- [ ] R123-I19 — R121 carry-forward: R118-062 — exact-head optimized freestanding build
- [ ] R123-I20 — R121 carry-forward: R118-063 — exact-head host Rust test matrix
- [ ] R123-I21 — R121 carry-forward: R118-064 — two-boot persistent graphical Files QEMU
- [ ] R123-I22 — R121 carry-forward: R118-065 — real ring3 two-process UDP QEMU
- [ ] R123-I23 — R121 carry-forward: R118-066 — QEMU boot and network matrix
- [ ] R123-I24 — R121 carry-forward: R118-067 — independent kernel/security review
- [ ] R123-I25 — R121 carry-forward: R118-068 — exact integration SHA workflow check
- [ ] R123-I26 — R121 carry-forward: R118-070 — protected post-merge main evidence
- [ ] R123-I27 — R121 carry-forward: R119-027 — group retirement through live network table
- [ ] R123-I28 — R121 carry-forward: R119-041 — scheduler fault and normal-exit integration
- [ ] R123-I29 — R121 carry-forward: R119-056 — cross-core IRQ scheduler delivery
- [ ] R123-I30 — R121 carry-forward: R119-069 — all mandatory checks green on final source SHA

## Readiness and next acceptance actions

- A source change is not a completed acceptance gate until its **exact commit SHA** passes the relevant Rust host and QEMU jobs. Track each CI job separately; skipped or cancelled is not success.
- Validate all final-head branch checks; the repository's `main` was **not protected** at initial R123 inspection. Require actual independent review, no admin bypass, no force-push/deletion, and strict status checks before merge.
- Carry forward R122's 57 unchecked primary tasks, the 30 inherited gates above, and the broader historical R120 obligations. Similar tasks mapped into R123 remain open until the original acceptance evidence is supplied.
- Hardware certification cannot be inferred from QEMU or host tests; physical multicore, IRQ/DMA, IOMMU, TPM, destructive power-loss and boot-sector restore remain OPEN.
- R123 implementation has one consolidated source commit; do not claim CI success, reviewer sign-off, production readiness or 100/100 completion without exact-head evidence.
