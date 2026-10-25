# R036 Batch 2 — Ten userland and desktop runtime foundations

**Integration branch:** `integration/r036-batch2-ten-tasks`

**Parent:** last green R034 main `25626297f2a311815bff218036dd5301829f4df7`.
**Dependency:** R035 Batch 1 PR #88 must pass and be merged with a successful main
build before the R036 batch can be rebased and merged. This branch intentionally
does not modify PR #88, `main`, or other branches.

All ten workstreams are part of **one coordinated commit**. The features below
provide allocation-free Rust policy/state-machine components with unit tests and a
shared host-only acceptance test. They are not claims that a complete desktop OS
is now booting into fully isolated user applications.

| Item | Implementation | Verification | Remaining target-level work |
|---|---|---|---|
| 11 | `batch2_init.rs`: dependency-ordered bounded service start plan | order, cycles, unknown dependencies | actual init PID, ELF loading, dependency startup on guest |
| 12 | `batch2_launcher.rs`: verified-image launch intent queue bound to PID incarnation | duplicate, unsigned and stale generations | ELF mapping, scheduler/syscall integration |
| 13 | `batch2_events.rs`: bounded PID/generation event broker | cross-PID, overflow, retirement | ring-3 IPC transport, authentication at syscall ingress |
| 14 | `batch2_windows.rs`: per-window ownership, geometry, revoke on exit | ownership, bounds, reuse | compositor service, actual rendering from ring-3 |
| 15 | `batch2_keyboard.rs`: route input to active authenticated window | no key leak, owner validation, queue capacity | actual PS/2 input path, lock/focus policy |
| 16 | `batch2_terminal.rs`: fixed-capacity line editing and allowlisted built-in command parser | editing, overflow, no arbitrary execution | attach physical keyboard, terminal renderer and safe command handlers |
| 17 | `batch2_files.rs`: read-only paginated Files listing, owner isolation | invalid names, foreign PID, paging and teardown | connect durable hierarchical guest VFS |
| 18 | `batch2_installer.rs`: verified-stage → consent → commit-policy / rollback state machine | consent, signature callbacks, rollback | true on-device crypto, installer transaction and disk writes |
| 19 | `batch2_settings.rs`: bounded per-user settings snapshots | invalid ranges and generation isolation | persistent settings, real UI controls |
| 20 | `batch2_gui_qa.rs`: visible-pixel screenshot comparison helper | exact matching, pixel differences, short buffers | QEMU screenshots, checked-in golden images, capture harness |

`batch2_acceptance.rs` exercises a coupled host-side flow:
init → verified launch intent → window ownership → focused physical-input
event → Terminal command → Files listing → IPC → Settings and installer
policy → framebuffer comparison. A second test checks process retirement,
preventing a reused PID from receiving old events.

## Acceptance and merge gate

1. Run `cargo test --offline --manifest-path rust/kernel_core/Cargo.toml` and
   freestanding x86-64 `no_std` Rust build.
2. Run the mandatory `IndianOS kernel boot (x86-64)` full QEMU workflow
   on the **exact** R036 branch head SHA.
3. Verify no CI status is skipped, missing, stale or failed. Any
   `Roadmap SHA Validation` check must also pass if available.
4. Independently review before merging. Do **not** auto-merge directly from a
   green host-only test; kernel CI must pass.
5. Rebase onto the final merged R035 Batch 1 `main` when it becomes green.
   **The rebased SHA invalidates old CI and must be tested afresh.**
6. Merge one reviewed R036 batch commit only after all gates pass and
   verify new main again.

No model-created commands are executed. No real disks are formatted, no remote
applications installed and no production or macOS/Windows/Linux compatibility
is claimed. This is safe Rust source scaffolding and cross-module tests only.
