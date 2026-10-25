# R038 Batch 4 — Ten compatibility and release-safety foundations

Development branch: `integration/r038-batch4-ten-tasks`.
Rebased onto merged R037 Batch 3 `main` `458d4b9a7360605bfdff9eb8ea5c1dfa66741d9e`.
**Integration**: R037 PR #90 passed full QEMU on retry and was merged.
R038 must pass its own rebase-head full QEMU suite and independent review.
No direct main writes from this draft PR.

| Task | Rust module | Bounded deliverable | Required real implementation |
|---|---|---|---|
| 31 AI catalog policy | `batch4_ai_catalog.rs` | offline scored selection with verified signer, architecture, risk and permissions gates | LLM integration, signed catalog and live app metadata |
| 32 Approval UX | `batch4_approval.rs` | consent bound to PID epoch, digest and permission mask | user-visible UI, authenticated action event |
| 33 Installer signature | `batch4_signature.rs` | trust-root revocation checks and fail-closed crypto callback | actual signature engine and digest verifier |
| 34 Install rollback | `batch4_rollback.rs` | staged / active / boot-attempt / rollback journal policy | durable transactional on-disk installer and power-loss test |
| 35 Linux compatibility | `batch4_linux_abi.rs` | allowlisted syscall classification and bounded pointer checks | real ELF Linux ABI, syscall translation, memory semantics |
| 36 Windows compatibility | `batch4_windows_pe.rs` | checked Win64 PE header identification | PE loader, Win32/NT API surface and legal runtime dependencies |
| 37 Android compatibility | `batch4_android.rs` | bounded APK ZIP local header and traversal rejection | Android runtime/ART, permissions, app graphics |
| 38 macOS lawful route | `batch4_macos_policy.rs` | portability, web app or licensed authorized remote host routing | user/host integration; NO unlicensed framework emulation |
| 39 Performance | `batch4_performance.rs` | overflow-safe tick samples, worst/mean budget gates | real on-device latency, throughput and perf regressions |
| 40 Release qualification | `batch4_release.rs` | exact SHA, checked evidence, platform coverage and independent-review gate | real signed CI attestation, multi-hardware test and audit |

`batch4_acceptance.rs` tests a combined host-only trust and install
decision path; all ten modules also have focused tests.
These are **scoped foundations** and are not proof that Linux, Windows,
Android and macOS applications actually run on IndianOS.

## Merge gates

1. Full host Rust tests: `cargo test --offline --manifest-path rust/kernel_core/Cargo.toml`.
2. Full no-std x86_64 kernel and QEMU boot suite for **exact commit SHA**.
3. After Batch 3 merges, rebase/integrate Batch 4 onto the tested Batch 3
   main, keeping **one** Batch 4 feature commit. The new SHA must be tested again.
4. Run exact-SHA policy, license/security review, and complete real main build.
5. Do not merge if any of those checks are missing, skipped or failed.
