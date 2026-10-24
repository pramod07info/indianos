# IndianOS 40-agent development assignment manifest

This file assigns work; it does not indicate implementations or tests have passed. Each agent branch is isolated; coordinator merges ten reviewed PRs into each phase branch. Phase branches must be rebased onto current main and retested before sequential merge. No force pushes or bypassed checks.

- Agent 01 (Phase 1): Durable filesystem extents. Deliver a bounded implementation, focused regression tests, and evidence of CI/QEMU where relevant. Do not claim success without observed results.
- Agent 02 (Phase 1): Guest package trust. Deliver a bounded implementation, focused regression tests, and evidence of CI/QEMU where relevant. Do not claim success without observed results.
- Agent 03 (Phase 1): Capability revocation generations. Deliver a bounded implementation, focused regression tests, and evidence of CI/QEMU where relevant. Do not claim success without observed results.
- Agent 04 (Phase 1): Service lifecycle. Deliver a bounded implementation, focused regression tests, and evidence of CI/QEMU where relevant. Do not claim success without observed results.
- Agent 05 (Phase 1): User-space compositor. Deliver a bounded implementation, focused regression tests, and evidence of CI/QEMU where relevant. Do not claim success without observed results.
- Agent 06 (Phase 1): UDP endpoints. Deliver a bounded implementation, focused regression tests, and evidence of CI/QEMU where relevant. Do not claim success without observed results.
- Agent 07 (Phase 1): PCI enumeration. Deliver a bounded implementation, focused regression tests, and evidence of CI/QEMU where relevant. Do not claim success without observed results.
- Agent 08 (Phase 1): A/B disk recovery. Deliver a bounded implementation, focused regression tests, and evidence of CI/QEMU where relevant. Do not claim success without observed results.
- Agent 09 (Phase 1): CI SHA gates. Deliver a bounded implementation, focused regression tests, and evidence of CI/QEMU where relevant. Do not claim success without observed results.
- Agent 10 (Phase 1): Dependency resolver. Deliver a bounded implementation, focused regression tests, and evidence of CI/QEMU where relevant. Do not claim success without observed results.
- Agent 11 (Phase 2): Init supervisor. Deliver a bounded implementation, focused regression tests, and evidence of CI/QEMU where relevant. Do not claim success without observed results.
- Agent 12 (Phase 2): Process launcher. Deliver a bounded implementation, focused regression tests, and evidence of CI/QEMU where relevant. Do not claim success without observed results.
- Agent 13 (Phase 2): Event broker. Deliver a bounded implementation, focused regression tests, and evidence of CI/QEMU where relevant. Do not claim success without observed results.
- Agent 14 (Phase 2): Window surface ownership. Deliver a bounded implementation, focused regression tests, and evidence of CI/QEMU where relevant. Do not claim success without observed results.
- Agent 15 (Phase 2): Keyboard routing. Deliver a bounded implementation, focused regression tests, and evidence of CI/QEMU where relevant. Do not claim success without observed results.
- Agent 16 (Phase 2): Terminal app. Deliver a bounded implementation, focused regression tests, and evidence of CI/QEMU where relevant. Do not claim success without observed results.
- Agent 17 (Phase 2): Files app. Deliver a bounded implementation, focused regression tests, and evidence of CI/QEMU where relevant. Do not claim success without observed results.
- Agent 18 (Phase 2): Package installer UI. Deliver a bounded implementation, focused regression tests, and evidence of CI/QEMU where relevant. Do not claim success without observed results.
- Agent 19 (Phase 2): User settings. Deliver a bounded implementation, focused regression tests, and evidence of CI/QEMU where relevant. Do not claim success without observed results.
- Agent 20 (Phase 2): Desktop regression tests. Deliver a bounded implementation, focused regression tests, and evidence of CI/QEMU where relevant. Do not claim success without observed results.
- Agent 21 (Phase 3): Driver isolation. Deliver a bounded implementation, focused regression tests, and evidence of CI/QEMU where relevant. Do not claim success without observed results.
- Agent 22 (Phase 3): AHCI probing. Deliver a bounded implementation, focused regression tests, and evidence of CI/QEMU where relevant. Do not claim success without observed results.
- Agent 23 (Phase 3): NVMe probing. Deliver a bounded implementation, focused regression tests, and evidence of CI/QEMU where relevant. Do not claim success without observed results.
- Agent 24 (Phase 3): Modern virtio. Deliver a bounded implementation, focused regression tests, and evidence of CI/QEMU where relevant. Do not claim success without observed results.
- Agent 25 (Phase 3): ACPI topology. Deliver a bounded implementation, focused regression tests, and evidence of CI/QEMU where relevant. Do not claim success without observed results.
- Agent 26 (Phase 3): Network reliability. Deliver a bounded implementation, focused regression tests, and evidence of CI/QEMU where relevant. Do not claim success without observed results.
- Agent 27 (Phase 3): Secure update. Deliver a bounded implementation, focused regression tests, and evidence of CI/QEMU where relevant. Do not claim success without observed results.
- Agent 28 (Phase 3): Crash diagnostics. Deliver a bounded implementation, focused regression tests, and evidence of CI/QEMU where relevant. Do not claim success without observed results.
- Agent 29 (Phase 3): Hardware test matrix. Deliver a bounded implementation, focused regression tests, and evidence of CI/QEMU where relevant. Do not claim success without observed results.
- Agent 30 (Phase 3): Permission audit. Deliver a bounded implementation, focused regression tests, and evidence of CI/QEMU where relevant. Do not claim success without observed results.
- Agent 31 (Phase 4): AI catalog policy. Deliver a bounded implementation, focused regression tests, and evidence of CI/QEMU where relevant. Do not claim success without observed results.
- Agent 32 (Phase 4): AI approval flow. Deliver a bounded implementation, focused regression tests, and evidence of CI/QEMU where relevant. Do not claim success without observed results.
- Agent 33 (Phase 4): AI package verifier. Deliver a bounded implementation, focused regression tests, and evidence of CI/QEMU where relevant. Do not claim success without observed results.
- Agent 34 (Phase 4): AI rollback. Deliver a bounded implementation, focused regression tests, and evidence of CI/QEMU where relevant. Do not claim success without observed results.
- Agent 35 (Phase 4): Linux ABI exploration. Deliver a bounded implementation, focused regression tests, and evidence of CI/QEMU where relevant. Do not claim success without observed results.
- Agent 36 (Phase 4): Windows API exploration. Deliver a bounded implementation, focused regression tests, and evidence of CI/QEMU where relevant. Do not claim success without observed results.
- Agent 37 (Phase 4): Android runtime exploration. Deliver a bounded implementation, focused regression tests, and evidence of CI/QEMU where relevant. Do not claim success without observed results.
- Agent 38 (Phase 4): macOS remote execution route. Deliver a bounded implementation, focused regression tests, and evidence of CI/QEMU where relevant. Do not claim success without observed results.
- Agent 39 (Phase 4): Performance benchmarks. Deliver a bounded implementation, focused regression tests, and evidence of CI/QEMU where relevant. Do not claim success without observed results.
- Agent 40 (Phase 4): Release qualification. Deliver a bounded implementation, focused regression tests, and evidence of CI/QEMU where relevant. Do not claim success without observed results.

## Coordinator acceptance

1. Confirm exact branch and SHA for each PR; review code, security implications and tests.
2. Integrate only completed work into its phase integration branch; reject placeholders.
3. Run mandatory Rust host tests, no_std kernel build, and required QEMU CI on integration head.
4. Merge one phase at a time into main only after passing checks and update the next phase against the new main SHA.
5. Run full mandatory CI again on final main and report actual workflow URLs.
