# IndianOS R025 — Twenty Major Developer Workstation Workstreams

**Source main**: `a6a75137526246dbfaa0c0e7dec1b8f9bbe7cfad`  
**Coordinator integration**: `integration/workstation-r025-twenty`  
**Policy**: 20 scoped development Git branches, 20 Rust modules, 60 focused host regression tests, two crate integration tests and one kernel QEMU guest acceptance smoke. One combined build. When all required checks pass, use one squash merge commit on `main`.

## Implemented changes

| Workstream | Subsystem | Functional scope |
|---|---|---|
| 01 | project-switcher | Real multi-project root switching, active project focus, invalid and stale root rejection |
| 02 | scoped-file-access | Generation-aware least-privilege root capability, rights expiry/revocation, actual read/write |
| 03 | workspace-recents | Real-file MRU project navigation, revision invalidation, owner-scoped recents |
| 04 | source-line-index | UTF-8-aware source line offsets and stale-safe line/column navigation |
| 05 | editor-search-session | Owner-bound literal source search, result pages, next-match wrap and current revision check |
| 06 | editor-batch-save | Multi-file atomic autosave staging, preflight revisions and single RamFs commit |
| 07 | editor-code-actions | Source-line indent/unindent/trim actions, preview and optimistic source version |
| 08 | workspace-symbol-links | Real-RamFs cross-file lexical symbol references with byte/line offsets and result limits |
| 09 | workspace-file-events | Bounded authenticated revision event journal for file create/update/delete |
| 10 | terminal-builtins | Typed read-only cat/stat/linecount/grep/head commands against real RAM source bytes |
| 11 | terminal-command-batch | Fail-atomic multi-command read-only terminal jobs with per-step output offsets |
| 12 | terminal-session-audit | Trusted command audit events with owner-generation isolation and failure counts |
| 13 | build-stage-graph | Toolchain-matched compiler/link/test/package DAG, exact dependency state and trusted CI report |
| 14 | build-artifact-manifest | Source-file content digest inventory, toolchain pin, totals and strict revision verification |
| 15 | test-suite-planner | Lexical real-Rust #[test] discovery from files, capacity guards and test shards |
| 16 | debug-breakpoint-manager | Source-line revision-bound debugger breakpoint registry and authenticated hit counts |
| 17 | debug-watch-snapshots | Typed trusted-debugger watch variable values, per-session isolation and value validation |
| 18 | window-command-routing | GUI/workbench typed Open/Search/FileStats dispatch with project ticket rights and actual source bytes |
| 19 | release-approval-policy | Build+source manifest+test evidence+independent reviewer release gate, no cryptographic claims |
| 20 | integrated-workstation-smoke | Integrated actual RamFs project navigation, editor, symbol search, terminal, build, test, debugger, UI and release guest smoke |

## Acceptance gates

- Roadmap SHA exact-head validation, S0 adversarial filesystem security regressions and host Rust tests.
- Orbit graphical desktop preview/QEMU, M1 native persistent VFS, M2 signed native-app/network integration and full x86_64 kernel boot/QEMU.
- Guest smoke in the existing `test-r041-workspace` gate must print `IndianOS: R025 developer-workstation project-build-debug acceptance PASSED`.
- Independent reviewer and coordinator workflow successful, exact SHA unchanged and no failing or pending required check. PR remains draft and **unmerged** until gates pass.

## Architectural boundaries

RAM file operations are real guest-state operations, not durable across reboot by themselves. Debugging hit signals, build/test results, and release gates require a trusted executor or debugger to supply evidence: they do not magically execute compiler jobs, hardware traps or signed binaries. Source symbol finding is lexical, not a full language server. This is not yet a complete desktop IDE GUI, real arbitrary shell, full native application catalog, remote Git client or OTA updater. The 20 branches denote 20 substantial workstreams and **not 20 simultaneously running independent AI agent processes**.
