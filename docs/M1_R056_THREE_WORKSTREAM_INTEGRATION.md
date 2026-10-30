# M1/R056 — three existing Rust subsystem improvements in one integration commit

Starting main: `357c7428fbbf47b93eeee81f207e399cdbad2a27` (R055 merged and authentic QEMU screenshot archive).

| Workstream | Production change | Added tests |
| --- | --- | --- |
| Durable file-handle I/O | Read into a fixed 72-byte private scratch buffer, never expose partly read or device-dirtied bytes to userspace on error; publish output and advance cursor only after a successful read. Existing I/O fault retirement remains intact. | Faulty block read leaves caller buffer intact, partial short/EOF read preserves tail |
| Graphical workstation service recovery | Transactionally transfer several owned windows to replacement owners using a shadow Broker. Prevalidate duplicate windows and old owner identity; scrub old-window queued input; preserve unrelated peer events; commit new grant epochs only after all transfers succeed. | Multi-window recovery, invalid batch fail-atomic |
| Persistent developer project editor | Replace a selected range of complete UTF-8 source lines with multiple lines through exactly one existing durable COW edit; support trailing-newline and unterminated final rows, owner/volume-generation validation, and capacity preflight. | Unicode source edit survives full remount; invalid/stale/oversized multi-line edits leave on-disk generation unchanged |

## Limits and validation

- Three **functional** Rust workstreams, **six** focused new Rust tests, and one consolidated commit and PR. No agent-slot files or placeholder functionality.
- **Independent coding agents actually launched: 0.** The connected GitHub API can submit code and run CI, but cannot spawn three autonomous Codex worktrees. Work was authored and integrated through one coordinator.
- Actual M1 milestone gates require exact-head no_std build, unit/security S0, QEMU graphical Files/Terminal reboot persistence, kernel boot and independent review.
- **No local Rust compiler** available in this interface; host/no_std and guest QEMU regressions are unverified until the PR checks pass.
- The project-line feature is bound to the bounded real HierVfs, not a general-purpose IDE or git.
- GUI batch recovery is a kernel policy primitive, not a completed ring-3 compositor; caller must serialize input dispatch and revoke old process grants at the lifecycle boundary.
- No physical-disk writes, no tests skipped, no branch rules waived, and no inferred M1/M2 completion percentages.
