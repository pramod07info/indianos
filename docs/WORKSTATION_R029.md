# R029 — IndianOS Developer Workstation (20 substantial workstreams)

**Base main:** `8c66cac6358982fa96172d4d946ea5bc85d9410e`  
**Coordinator integration branch:** `integration/workstation-r029-twenty`

## Functional deliverables

| Agent | Real feature / functionality |
|---|---|
| 01 | Atomic recoverable trash and safe restore on real project files |
| 02 | Conservative three-way real-source file merge without overwriting conflicts |
| 03 | Bounded byte-block diff for actual guest files |
| 04 | Reverse Rust use-import change impact inventory from project files |
| 05 | Project recent activity feed by actual file generations |
| 06 | Optimistic real-file source undo and redo stack |
| 07 | Session-owned UTF-8 clipboard selection and live source paste |
| 08 | Real Rust line comment toggle with stale file protection |
| 09 | Cross-file source search with exact hit path, line and byte offsets |
| 10 | Safe permission-checked terminal command dispatcher executing file cat/stat/wc/grep/head |
| 11 | Private bounded terminal command history with duplicate suppression and search |
| 12 | Bounded terminal scrollback pager with complete line boundaries |
| 13 | Real source manifest/checksum cache invalidation by toolchain |
| 14 | Strict diagnostic parser grouping compiler warnings/errors with release block |
| 15 | Trusted per-test failure evidence records and verified clear |
| 16 | Authenticated live-source revision-bound debugger breakpoints |
| 17 | Trusted typed integer debugger watches with arithmetic expressions |
| 18 | Real viewport tiling geometry presets for compositor integration |
| 19 | Guest filesystem-backed workspace layout save/load with owner+integrity checks |
| 20 | Integrated QEMU guest file/editor/terminal/build/debug/layout acceptance |

## Architecture, validation and integration
- Each of 20 independent `agents/NN-r029-*` Git branches has one substantial Rust source module and two regression tests (40 module tests), plus two R029 cross-module tests.
- QEMU guest acceptance invokes `workstation_r029::agent_20::smoke()`, with serial marker `IndianOS: R029 real guest editor terminal build debugger layout PASSED`.
- Coordinator combines all 20 source modules and the actual kernel guest test in one initial integrated commit, then runs one combined CI check set rather than 20 repeated branch builds.
- Require **exact-head** Roadmap SHA, S0 Security Gate, independent reviewer, Orbit GUI/QEMU, M1 disk/VFS integration, M2 package/network integration, and full x86-64 kernel/QEMU. Fix any failure on coordinator and rerun. Do not merge pending/failed builds.
- When all required checks pass, **squash merge** so the complete R029 lands as one `main` commit.

**Scope:** These are real RamFs-backed file and editing operations, but RamFs data is volatile without M1 persistent-disk integration. Session file save uses this guest VFS, not power-loss durability by itself. Debug controls/test failures/build manifest consume trusted signals and do not implement a native debugger engine or compiler. Window tiling produces geometry for GUI integration, not new drawn graphics. Git workstreams are not 20 independently executing AI coding agents.
