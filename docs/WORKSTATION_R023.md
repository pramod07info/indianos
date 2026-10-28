# R023 — IndianOS Developer Workstation integration (20 large workstreams)

- **Base main**: `8c0a4c2eb75c8d423e462d1155d6352821d06153`
- **Coordinator branch**: `integration/workstation-r023-twenty`
- **Delivery policy**: 20 independently scoped Git development workstreams assembled as a single initial integration commit, one full combined build and squash-merge to one mainline commit after green CI.
- **Tests**: 60 module-specific Rust host/adversarial tests and 2 cross-module integration tests, plus conditional `test-r041-workspace` guest smoke.

## Engineering implementation

| Workstream | Subsystem | Implemented features |
|---|---|---|
| 01 | project-scaffold | Real Rust CLI/library/web starter source files created atomically in RamFs, collision and capacity guards |
| 02 | file-batch-transactions | Transactional actual RamFs create/replace/delete with stale-generation checks and fail-atomic rollback |
| 03 | workspace-grep-replace | Literal cross-file find/replace across RAM-backed source files, preview and capacity rollbacks |
| 04 | editor-bracket-nav | Bracket matching/validation over actual source bytes, quote/escape-aware lexical navigation |
| 05 | editor-line-transforms | Tab expansion, trailing-space trimming and CRLF normalization on real file revisions |
| 06 | editor-history-groups | Project-wide grouped undo/redo with actual RamFs image restore and identity protection |
| 07 | workspace-state-restore | Multi-tab and layout session restoration with real file revision validation and unsaved-close guards |
| 08 | file-conflict-detect | Actual source-file three-way merge classification, fail-closed conflict and optimistic version protection |
| 09 | manifest-dependency-check | Dependency-ordered source build graph and transitive invalidation on package updates |
| 10 | build-pipeline-retries | Real job state machine with dependencies, deadline, bounded retries, exponential backoff and trusted executor |
| 11 | terminal-tokenizer | Shell-like quoted-argument tokenizer, escaping and bounded display rendering (not command execution) |
| 12 | terminal-commands | Real read-only kernel RAM filesystem terminal builtins pwd/ls/cat/stat with bounded output |
| 13 | terminal-env | Authenticated shell session variables with generation isolation, revisioned set/get/unset |
| 14 | console-output | Sanitized per-process sequenced console stdout/stderr buffers, ring pagination and owner filtering |
| 15 | test-output-parser | Rust test output parsing, duplicate protection, failure summaries and release gating |
| 16 | diagnostic-quickfix | Compiler diagnostic exact-line quick-fix preview and atomic real-file source replacement |
| 17 | workspace-search-rank | Ranked filename and content search from current project file tree, revision-guarded results |
| 18 | ui-command-dispatch | Permission-bound UI command admission and actual read-only Files/Terminal execution |
| 19 | release-checkpoints | Actual source-version, build, test and independent review prerequisites for publishing metadata |
| 20 | guest-acceptance | Integrated real source scaffold, edit, format, terminal, quick-fix, build/test and release QEMU smoke |

## Acceptance

1. All 20 modules compile in the Rust kernel crate and run with host tests.
2. `no_std` kernel compilation and S0 adversarial filesystem/security policy test pass.
3. Opt-in x86_64 QEMU guest boot reaches `IndianOS: R023 real source terminal-test-release workflow PASSED`, executing actual `agent_20::smoke()` over kernel RAM-backed file bytes.
4. Orbit R001 GUI/QEMU smoke, M1 durable-VFS integration, M2 signed native-app acceptance, Roadmap SHA and full kernel/QEMU boot pass against the exact coordinator SHA.
5. Independent review is successful and the base main ref is not stale. Open draft PR stays **unmerged** until all gates pass. Use **squash merge** for a single R023 mainline commit.

## Boundaries

These changes add substantially more **functional source-file and read-only terminal operations** over real `RamFs` than policy-only metadata, but are not a full graphical IDE, arbitrary shell execution, compiled binary toolchain, fully persistent project storage, real app marketplace, live remote Git transport or actual signed OTA installer. Local snapshot recovery is RAM only. Compiler/build/test statuses must be supplied by a trusted executor. Cryptographic artifact signing and remote repository integrations remain separate work.

"20 agents" refers to 20 named Git feature branches/workstreams and not twenty independently running AI development agents.
