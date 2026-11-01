# IndianOS M1/R077 — Five-workstream integration

Parent main: 3a81f54335bd20fc18accc81449ab9fdf4a9b607

This batch changes existing production Rust services. Five *logical*
workstreams were integrated by one coordinator; do not infer that five
independent coding agents were started.

| Workstream | Task | Production behavior |
| --- | --- | --- |
| A: Editor | 01 | Owner-bound per-document undo with independent document state |
| A: Editor | 02 | Owner-bound per-document redo and status inspection |
| B: Projects | 03 | Cursor-based paginated project-wide UTF-8 literal search |
| B: Projects | 04 | Unicode-boundary-checked, capacity-safe project source windows |
| C: Terminal | 05 | Permission-checked matchpage offsets for persistent text |
| C: Terminal | 06 | Protected-path-checked copyrange into new durable files |
| D: Desktop editor | 07 | Authenticated multi-document status/read routes |
| D: Desktop editor | 08 | Authenticated multi-document undo/redo routes |
| E: Desktop projects | 09 | Authenticated project search pagination route |
| E: Desktop projects | 10 | Authenticated project Unicode window route |

Boundaries: allocation-free, existing eight-object disk volume, only
direct project files, no shell pipes or graphical editor claims.
Terminal matchpage uses whitespace-delimited literal search terms.

Tests cover independent source undo/redo, stale identities, closed/invalid
slots, UTF-8 boundaries, multi-page cursors, short outputs, grant expiry,
protected destinations, durability after real VFS remount, and desktop
routing. Build, tests and QEMU pass are **not claimed** until confirmed by
GitHub Actions. One source commit and one PR; merge only when all mandatory
checks are successful.
