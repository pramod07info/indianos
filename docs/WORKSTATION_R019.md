# R019 — IndianOS developer workstation, 20 integrated workstreams

**Baseline:** `a7f7167af0ac42de1be98bdb1dc7b8f9449156d4`
**Coordinator:** `integration/workstation-r019-twenty`
**Rule:** one large integration commit triggers the full validation PR; no 20 separate full QEMU builds.

R018's 40 foundational services were already on `main`; R019 adds actual connected workflow APIs rather than duplicating R018's project, edit and job state methods.

## Workstreams
| Agent | Component | Functionality |
|---|---|---|
|01|Project files|Authenticated, versioned real RamFs file write/append/read/hash|
|02|Document editor|Load/save actual file content, replace one occurrence, checksum|
|03|Source search|Cross-file literal hit positions, atomic result publishing|
|04|Compiler diagnostics|Parse file/line/column/severity from bounded text|
|05|Build graph|Dependency-ordered compile/test/package task lifecycle and retry|
|06|Build cache|Content-addressed task artifact reuse, eviction and invalidation|
|07|Breakpoints|Source location breakpoints, hit counts, enable/disable|
|08|Debugger session|Trusted attach/resume/stop event/teardown|
|09|Change staging|Owner-scoped file digest staging, commit index|
|10|Source diff|Line-by-line additions/removals with bounded output|
|11|Terminal history|Command history, exit status, rotating ring and per-owner clear|
|12|Command palette|Case-insensitive search with explicit capability requirements|
|13|File navigator|Real RamFs path tree, recursive directory filtering and byte counts|
|14|Workspace tabs|Split-pane assignments, focus, unsaved-change close protection|
|15|Extension permissions|Signer gate, requested/approved rights, disable/revoke|
|16|Preview/hot reload|Build revision-gated preview lifecycle, supervisor verification|
|17|HTTP inspector|Owner-scoped metadata, failed requests and privacy-safe samples|
|18|Package lock|Deterministic dependency versions and digests validated against catalog|
|19|Deployment gate|Signed/tested manifest metadata with independent reviewer approval|
|20|Integration acceptance|Edit source, search, ordered tasks, stage artifact, gate release|

## Gates
- All Rust host test modules (at least 60 adversarial and positive tests) pass.
- `rust/kernel_core` compiles in actual `no_std` kernel configuration.
- Opt-in QEMU `test-r041-workspace` logs `IndianOS: R019 developer workstation edit-build-stage-approval PASSED`.
- S0 Security Gate, SHA validation, full kernel/QEMU build, M2 developer acceptance, independent review pass on **same head**.
- Only after green CI and current main head can coordinator be considered for merge.

**Limits:** source editing is RAM-backed; no native code compiler execution, full GUI IDE, real Git repository changes, actual HTTP packet capture, hardware debugger, live extension runtime, deployment to cloud or bootable universal compatibility is implemented here. Policy and metadata gates are NOT cryptographic signature verification by themselves.
**Orchestration honesty:** Branches called agent-01..20 represent scoped Git development workstreams, not 20 actual autonomous worker processes.
