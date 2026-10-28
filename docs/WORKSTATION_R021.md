# IndianOS R021 — 20 major Developer Workstation workstreams

**Baseline main:** `f888f4df1c931eb101f15e7065d78ce32aa7ba25`  
**Coordinator branch:** `integration/workstation-r021-twenty`  
**Commit policy:** one combined source commit (20 workstream branches assembled into one Git tree) with only one integration PR and one full CI matrix per head.

## Implemented work

| Workstream | Module | Substantive scope |
|---|---|---|
| 01 | project-manifest | Dependency-ordered package manifest and locked source digests |
| 02 | autosave-transactions | Generation-scoped atomic save of multiple actual RamFs documents |
| 03 | file-watch-events | Trusted filesystem change journal, revision cursor and subscription filters |
| 04 | rust-outline | Rust source symbol outline from real workspace files |
| 05 | identifier-completion | Frequency-ranked identifier completion of actual project source |
| 06 | build-resource-quotas | CPU/RAM-limited compile job reservations and deadline handling |
| 07 | job-log-stream | Sequenced, sanitized stdout/stderr-like job logs and history |
| 08 | static-lint | Source lint (line size, tabs, trailing spaces, TODO) over real RamFs |
| 09 | test-result-registry | Retry-aware test results, failures, durations, flaky-test detection |
| 10 | diagnostic-navigation | Compiler diagnostics mapped to exact current source-file revision |
| 11 | debug-values | Typed debugger value snapshots with owner generation privacy |
| 12 | debug-callstack | Debugger call stacks with bounded authenticated frame capture |
| 13 | merge-conflict-planner | Three-way source merge conflict and auto-resolution planning |
| 14 | branch-review-gates | Independent review + exact-head CI + base revision checks |
| 15 | window-focus-policy | Authenticated modal/active-window focus management using real Windows |
| 16 | task-history-search | Searchable per-project build/run history with retention and filtering |
| 17 | launch-configurations | Signed-artifact launch preflight, rights, timeout and CPU/RAM budgets |
| 18 | toolchain-versions | Signed toolchain catalog, pinned versions, ABI and channel authorization |
| 19 | update-channel-rollout | Staged signed OTA descriptor/cohort policy, pause and withdrawal |
| 20 | full-integration-smoke | Integrated QEMU-safe source save, outline, completion, lint, build, test, review |

## Verification contract

- 20 modules each expose multiple safe feature operations and three positive/security regression tests; module integration adds two more.
- `cargo test` with all newly linked modules; compile native no_std kernel and run exact QEMU guest smoke marker `IndianOS: R021 developer workstation autosave-lint-build-test PASSED`.
- Rust policy, S0, Roadmap SHA Validation, M1 persistence integration, M2 signed app acceptance and kernel/full QEMU plus independent reviewer must pass at the same exact coordinator SHA.
- Never claim a build is passing before GitHub Actions confirms it; do not merge while draft or required checks are red.

## Product scope truth

This adds actual RAM-backed source files, atomic autosave, lexical navigation and code hints, a safe metadata-oriented build/test/debug pipeline and focused permission/CI gates. It does **not** run native compiler binaries, provide a true semantic Rust parser/language server, implement physical disk persistence for these new paths, launch a production GUI IDE, install over-the-air updates, contact a Git remote, or constitute a fully complete Developer Workstation. Toolchain/OTA signer claims require cryptographic verification at a separate trusted layer.

20 named Git branches are 20 scoped workstreams committed directly through the available GitHub connector, **not 20 autonomous independently running worker processes**.
