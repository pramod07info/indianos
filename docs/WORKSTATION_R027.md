# IndianOS R027 — 20 major Developer Workstation workstreams

Base `main`: `c04328f0cf553cc01b44b35c2b5743a8b524e50e`

Integration: `integration/workstation-r027-twenty`

Process: twenty independently scoped Git workstream branches each implementing real Rust functions and three regression tests, assembled by coordinator into one initial integration commit. Run one combined multi-workflow CI set. Only squash merge R027 into a **single main commit** when every required workflow passes.

| Agent | Workstream | Functional changes |
|---|---|---|
| 01 | project-file-index | Real RamFs project file inventory, nested depth, sorted paths, byte aggregates and bounded results |
| 02 | project-rename-transaction | Atomic optimistic multi-file project rename with collision preflight and snapshot publication |
| 03 | project-usage-policy | Quota-aware real file writes per project, separate file/byte budgets and stale rejection |
| 04 | file-content-checksum | FNV-1a file content integrity fingerprints bound to real metadata versions |
| 05 | source-patch-engine | Exact-preimage UTF-8 source patch engine with lost-edit prevention and bounded previews |
| 06 | editor-bookmark-store | Session-owned file+line bookmarks validated against actual source revisions |
| 07 | editor-fold-index | Lexical Rust multi-line brace folding range index with revision invalidation |
| 08 | symbol-rename-preview | Exact identifier hit positions and lexical symbol rename preparation |
| 09 | project-template-generator | Transactional real Rust source and README project template creation |
| 10 | terminal-alias-resolver | Generation-isolated typed command aliases with validation and chain resolution |
| 11 | terminal-scrollback | Owner-bound bounded terminal scrollback, newest page navigation and literal search |
| 12 | terminal-permission-gate | Least-privilege terminal source read/write/delete across project-scoped tickets |
| 13 | build-dependency-plan | Trusted build DAG steps with dependency, toolchain and failure gates |
| 14 | artifact-verification | Real source digest/revision manifest and toolchain verification for build reuse |
| 15 | test-result-explorer | Trusted test result registry with retries, duration and release gate |
| 16 | debug-callstack-view | Authenticated trusted debugger callstack frames, lookup and revisions |
| 17 | debug-expression-evaluator | Safe integer debugger arithmetic, comparisons, overflow and invalid input checks |
| 18 | window-layout-engine | Bounded desktop window geometry, focus and movement at a specified screen resolution |
| 19 | workspace-snapshot-import | Integrity-checked per-user volatile workspace snapshot export/import |
| 20 | integrated-qemu-acceptance | Combined real QEMU guest test of project file operations, source editor, terminal, build, test, debugger and window layout |

Acceptance: 60 unit tests, two integration tests, and x86-64 QEMU smoke in existing `test-r041-workspace`, requiring exact serial marker `IndianOS: R027 real source editor terminal build debug acceptance PASSED`.

Required exact-head checks: Roadmap SHA; S0 security; Independent Reviewer; Orbit graphical QEMU preview; M1 durable VFS/reboot test; M2 app and network integration; full x86_64 kernel/QEMU boot. Never merge while checks are pending or failed.

**Boundaries:** Real file edits execute against in-memory `RamFs`, not reboot-durable disk. Source outline and fold are lexical (not full compiler semantic analysis); window geometry is not a graphical compositor. Build/test/debugger policy modules consume externally trusted evidence; they do not implement a full native compiler, hardware trap debugging, arbitrary shell or native third-party software compatibility. Snapshots remain volatile in RAM. 20 workstreams do not mean 20 independent autonomous agents running simultaneously.
