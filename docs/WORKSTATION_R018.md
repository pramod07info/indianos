# R018 Developer Workstation — 40 bounded operations

Baseline SHA: 342cbda3463ed6adcd31cdc3188a99d0218e78c4
Branch: integration/workstation-r018-40-workstreams

Five independent no-heap, no_std service-side tracks. The 40 numbered operations below are implemented APIs, NOT 40 separately launched AI coding agents.
All state is in memory; no graphical editor, disk persistence, process runner, automatic updater, physical GPU, or universal app support is claimed.
Caller IDs MUST be supplied by trusted kernel ingress, not a UI string.

## Workstream ledger

### Projects
- 01: create metadata
- 02: rename at revision
- 03: tag at revision
- 04: archive
- 05: pin/unpin
- 06: set readonly
- 07: restore
- 08: remove archived metadata

### Editor
- 09: load bytes
- 10: insert bytes
- 11: replace range
- 12: delete range
- 13: append bytes
- 14: one-step undo
- 15: find sequence
- 16: copy exact range

### Build job queue
- 17: submit metadata
- 18: reprioritize
- 19: start next deterministic
- 20: complete
- 21: fail with reason
- 22: retry within budget
- 23: cancel pending
- 24: inspect own job

### Service plans
- 25: select Desktop/Developer/Server/AllInOne
- 26: register trusted metadata
- 27: enable opt-in
- 28: disable stopped
- 29: mark started
- 30: mark stopped
- 31: record quota
- 32: query status

### Diagnostics
- 33: append event
- 34: lookup sequence
- 35: latest own event
- 36: count event kind
- 37: count after checkpoint
- 38: freeze journal
- 39: thaw journal
- 40: purge owner

## Single integrated build contract

- One integration source commit with a Rust module, lib.rs registration and this ledger. Do not build 40 individual PRs.
- Run offline Rust host unit tests, no_std x86_64-unknown-none compilation and Rust source audit.
- Run entire existing full kernel/QEMU CI, security S0 and Roadmap SHA Validation for the identical PR head SHA.
- Review auth, stale revisions, capacity overflow, rollback safety, and owner isolation before merging.
- Keep this PR draft until checks and independent review are green. Main branch must not be overwritten or force-pushed.

## Scope gaps / next production-sized integration

- Bind project metadata and editor to authenticated ring-3 sessions, actual GUI compositor and durable filesystem.
- Run real isolated compilers/debuggers/terminal processes; the job queue here only schedules metadata.
- Connect optional services and quotas to verified installation packages and actual supervisor enforcement.
- Persist safe diagnostic metadata with retention rules and test cross-reboot recovery.
- A 40-worker autonomous coding environment separately requires runners, model access, cost controls and code-review gates. This source commit does not launch one.
