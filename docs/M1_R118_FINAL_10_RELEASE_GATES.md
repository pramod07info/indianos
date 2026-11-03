# IndianOS R118 — final 10 pending acceptance tasks (commit 6/6)

Cohort IDs: R118-091, R118-092, R118-93, R118-94, R118-95, R118-96, R118-97, R118-98, R118-99, R118-100.

This checklist is a **release hold**, not a claim of kernel completion. This commit adds automatic CI coverage for the exact-head GitHub evidence verifier and records what additional proof is required.

| R118 ID | Actual exit criterion | Status |
|---|---|---|
| 091 | Final exact-commit two-cold-boot persistent graphical Files with checksum | OPEN |
| 092 | Independent review, protected merge SHA and QEMU vs hardware evidence | OPEN |
| 93 | Exact-head full Rust host test suite, not only specific cargo test filters | OPEN |
| 94 | Exact-head optimized, freestanding no_std target build | OPEN |
| 95 | QEMU persistent Files cold reboot evidence and exact-head boot logs | OPEN |
| 96 | Two actual ring3 tasks and network port ownership visible in QEMU run | OPEN |
| 97 | QEMU boot epoch torn-write, flush-failure and rollback fault-matrix proof | OPEN |
| 98 | All required GitHub workflow checks successful on the last final SHA | OPEN |
| 99 | Independent reviewer and confirmed branch protection | OPEN |
| 100 | Merge main SHA verified; physical-hardware evidence gaps published | OPEN |

## CI-only scope
The workflow `.github/workflows/r118_release_evidence.yml` executes the Python release-evidence checker tests on pull request changes. It does not approve a PR and is not a substitute for an independently reviewed and branch-protected merge.

## Hardware qualification remains outstanding
- True multicore scheduler IPI and file/network teardown, with two active hardware APs
- Physical NIC interrupt/port generation and multiple PCI devices under IOMMU DMA remapping
- Kernel-calibrated monotonic NIC clock and real delayed lease expiry
- Immutable measured boot trust source provisioned by UEFI/TPM, not a test observer
- Real abrupt power cut and hardware boot sector recovery; do not use a user machine for destructive tests

## Merge rules
Do not claim passed checks based on any prior commit in this PR. The final head must pass the host/security/QEMU/optimized kernel workflows and release evidence unit checks. Confirm independent approval and branch protection, then merge only once all applicable required gates are satisfied. Publish actual merged main SHA after merge.
