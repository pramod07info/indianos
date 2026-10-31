# IndianOS R068 — Five substantial workstation workstreams, one commit

Base main SHA: `8d360d97a833ce048b208da51a5e60b5c0894191`; includes merged PR #692 plus genuine QEMU screenshot archive.

| Workstream | New real functionality | New tests |
|---|---|---:|
| Project Workspace | Exact-byte optimistic compare-and-save, including generation lock, no-op behavior and single durable COW overwrite | 3 |
| Authenticated Desktop | New protected routes for editor comment/uncomment, complete source-line replacement, multi-range refactors and optimistic project saving | 3 |
| Window Management | Transactional area-edge snapping of multiple owner-validated surfaces, idempotent geometry and epoch behavior | 2 |
| Terminal Editing | Forward-word deletion at the caret with no command submission, clipboard change or history mutation | 3 |
| Durable Terminal | Unicode-aware `wc /file` counts lines, words and exact UTF-8 bytes from mounted disk with safe output bounds | 3 |

**Total: 14 new Rust regression tests, 5 existing source modules and this document.**
Five engineering workstreams implemented by a single GitHub coordinator; 0 independent Codex coding workers launched.

Accept only after exact-head Rust host/no_std tests, combined M1, S0 security, QEMU normal boot, desktop Files/Terminal persistency after guest reboot, independent review and required branch protection. Keep PR Draft while checks are pending. Do not claim any new M1 gate closed without exact guest evidence.
