# PR #782 — WS71–WS90 kernel-first hardening (six coordinated workstreams)

## What this implementation changes

Twenty **new source-level operations**, with targeted host tests in six
existing Rust kernel modules. These do not claim physical PCIe MMIO,
driver binding, interrupt masking, hardware DMA/IOMMU, power-loss recovery,
independent review, or completion of the M1 acceptance gates. Caller-provided
hardware evidence MUST come from trusted firmware, drivers and CI sources.

Six workstreams means six logical code groups, **not six independent,
long-running coding agents**. One integration commit reuses PR #782's head.

| ID | Area | Implemented policy | Required negative / positive proof |
|---|---|---|---|
| WS71 | ECAM | Checked full aligned physical aperture | zero/forged/wrapping base denied |
| WS72 | ECAM | Unique segment+bus firmware window lookup | ambiguous/invalid unselected window denied |
| WS73 | ECAM | Atomic multi-register numeric config address calculation | bad width or same-function overlap leaves output unchanged |
| WS74 | ECAM | Validate exclusion of firmware/MMIO reserved regions | partial overlap and overflow denied |
| WS75 | PCIe topology | Validate public scan summary, counts and unique BDFs | forged counts and repeated device rejected |
| WS76 | PCIe topology | Exact bus/device/function read-only lookup | unsupported slot or forged state rejected |
| WS77 | PCIe topology | Atomic per-bus accelerator snapshot | insufficient capacity preserves destination |
| WS78 | Persistent ACL | Trusted root-only change owner policy | non-root, corrupt mode denied |
| WS79 | Persistent ACL | Owner-constrained change group policy | owner cannot choose unrelated GID |
| WS80 | Persistent ACL | Restriction-only chmod | mode widening and peer request denied |
| WS81 | Persistent ACL | All-directory SEARCH and leaf-rights admission | invalid and inaccessible ancestors denied |
| WS82 | Crash log | PID-generation and fault-vector-filtered receipt copy | short buffer not partially written |
| WS83 | Crash log | Exact owner retained-sequence interval count | peer and stale-generation failures |
| WS84 | Crash log | Atomic owner uptime-window snapshot | invalid time window, short capacity denied |
| WS85 | Hardware qualification | Reject unrecognized pass/fail/skip evidence bits | unknown flags cannot certify |
| WS86 | Hardware qualification | Distinct-platform exact-SHA coverage matrix | duplicate QEMU cannot stand in for physical |
| WS87 | Hardware qualification | Explicit nonzero subsystem-mask acceptance | missing/skip/fail or invented bits denied |
| WS88 | Retirement gate | Read authenticated registered BSP identity | unbound gate never implies CPU 0 |
| WS89 | Retirement gate | RAII-scoped non-reentrant BSP operation | held permit excludes concurrent admission |
| WS90 | Retirement gate | Exact permit, BSP identity and masked interrupts gate | wrong gate/CPU/unmasked denied |

## Integration

- Changed production modules: \`hardware_mcfg.rs\`, \`hardware_pcie.rs\`,
  \`fs_permissions.rs\`, \`batch3_crashlog.rs\`,
  \`batch3_hardware_matrix.rs\`, \`retirement_gate.rs\`.
- Host command: \`cargo test --manifest-path rust/kernel_core/Cargo.toml --lib\`.
- Exact-head GitHub CI: security, kernel boot, M1 persistent desktop,
  combined reliability, QEMU boot-epoch and real two-vCPU checks.
- New stricter helpers are **opt-in until wired to trusted production
  call sites**. Host tests validate helper behavior only.
- Reject claims of actual physical IOMMU isolation, real interrupt masking
  or independently attested hardware qualification without evidence.
- Do not merge until branch protections, exact SHA CI and external review
  requirements are satisfied. Main SHA must be reverified after merge.
