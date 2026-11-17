# IndianOS final ten selected M1 master IDs (081–090 task-targeting batch)

Date: 2026-11-17. Base PR: #784. Parent SHA: `200d5019e2c874563f25a0a7004c1b566358b67d`.

**Ten previously untouched master task IDs:** 047,048,049,050,053,054,068,069,079,081.
The master checklist records acceptance, not a score for attempted source changes.
This commit does not change any of its 90 acceptance checkboxes.

| Workstation | Master IDs | Integrated code | Scope remaining for full gate acceptance |
| --- | --- | --- | --- |
| W1 desktop settings/task manager | 047,048 | Generation-aware settings GUI compare-and-set, bounded PID1-only runnable inventory | Real GUI, process metrics, safe task management actions |
| W2 media and desktop utilities | 049,050 | Bounded PNG/WAV headers, checked screenshot pixel copy from genuine buffers, overflow-checked calculator | Actual media playback/render, capture and real-time clock |
| W3 input | 053,054 | HID boot report decoder routed through exact-generation focused keyboard, atomic trusted pointer-IRQ focus transfer | Physical xHCI/USB host, endpoint enumeration, IRQ and DMA/actual mouse delivery |
| W4 verified boot/physical UEFI | 068,069 | Extra firmware-enforced Secure Boot trust contract and multi-machine/commit UEFI evidence gate | Audited firmware/TPM driver and tested physical UEFI PCs, independent signed attestations |
| W5 suspend/resume | 079 | Page-aligned SHA-256 memory-snapshot digest and exact CPU/device/boot epoch revalidation | Actual S3/S0ix suspend/resume, CPU restoration and physical power tests |
| W6 removable/encryption | 081 | Fail-closed generation-pinned removable-media authenticated-encrypted-backend boundary, rejects plaintext fallback | Real AEAD implementation with key handling, removable driver, hardware unplug and power-loss qualification |

Six GitHub Actions validation workstations run `m1_batch05_w1` ... `w6`
plus inherited test cohorts and freestanding Rust compile at the exact PR SHA.
These matrix jobs are **not six independent coding agents**.

**Important:** All 90 IDs have source slices targeted after this batch.
**0 of 90 have been newly marked fully accepted by these batch ledgers.**
This is NOT proof of a bootable production desktop, functional USB/xHCI,
Secure Boot, physical PC boot, system suspend/resume, or storage encryption.
Keep PR #784 draft until exact-head QEMU, S0 security, integration,
hardware-specific acceptance and independent review can support merge.
