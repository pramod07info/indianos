# R037 — M1/M2 persistent Developer Workstation integration

**Baseline:** merged R036 PR #605, main `2e69127b7d1a50a5bbf70a7e16e9ab9fb10a2453`.

This batch implements new real **durable HierVfs project editing APIs** by deepening `m1_project_workspace.rs`; it deliberately does **not** create twenty tiny `agent_XX.rs` wrappers. Individual workstreams are engineering scopes, not evidence of twenty independent autonomous AI agents.

## 20-workstream delivery ledger

| # | Workstream | Status this batch |
|---|---|---|
| 1 | Persistent project tree session | Existing foundation, extended |
| 2 | Create durable project & file | Existing foundation, extended |
| 3 | Durable read/open source | Existing foundation, extended |
| 4 | Durable source save | Existing foundation, extended |
| 5 | Atomic source range edit | Implemented/tested slice |
| 6 | Unicode line/column navigation | Implemented/tested slice |
| 7 | Project copy and rename | Implemented/tested slice |
| 8 | Project source search | Implemented/tested slice |
| 9 | Source line count and checksum | Implemented/tested slice |
| 10 | Append to durable source | Implemented/tested slice |
| 11 | Duplicate source safely | Implemented/tested slice |
| 12 | Filesystem remount QA | Implemented/tested slice |
| 13 | Normal production desktop boot/PID1 | Pending — no claim of completion |
| 14 | Ring-3 Files/Terminal syscall integration | Pending — no claim of completion |
| 15 | Userspace compositor protocol | Pending — no claim of completion |
| 16 | Physical keyboard/mouse input | Pending — no claim of completion |
| 17 | Signed package guest installation | Pending — no claim of completion |
| 18 | Actual third-party native app execution | Pending — no claim of completion |
| 19 | Graphical Developer Workstation UI | Pending — no claim of completion |
| 20 | End-to-end M1+M2 acceptance | Pending — no claim of completion |

## Actual new source functionality

- Bounded UTF-8-only persistent file open, no leaking partial output on undersized buffers.
- Source patch with UTF-8 character boundaries, stale-volume revision validation and COW overwrite.
- Append to real on-disk project file and project-leaf duplicate without overwrite.
- Unicode-aware line/column lookup and bounded project source search with staged output.
- Line counts and FNV source checksums (non-cryptographic; never use for signature verification).
- Tests cover real HierVfs disk remount, stale callers, stale filesystem generation, invalid UTF-8 offsets, search capacity, duplicate collision and source integrity.

## Acceptance and exclusions

Host Rust tests, no_std build, Security S0, M1 real QEMU durable VFS/Files, M2 package/network, Orbit graphical QEMU and full kernel boot are required at exact PR head. No production desktop or third-party installer exit gate is claimed. Only merge when all required CI and code review pass; squash to one commit on main.
