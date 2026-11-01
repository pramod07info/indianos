# IndianOS M1/R073 — Five functional developer workstation streams

Base: R072 PR #702 merged at 36130fe76d2e7cfdd45cbf52f77648c351cc4832, including genuine QEMU screenshot archive on main 4d72a329d19a0e45f9d41213b32c84915902c802.

## Five production workstreams

1. **Source file hygiene.** Owner-bound DiskEditor removes the UTF-8 BOM (only at file beginning) and adds a final LF when missing; both changes form one undoable and redoable unsaved edit, persisted only through the existing authenticated save. Invalid UTF-8 and NUL source fail without mutation.
2. **Optimistic UTF-8 source selection update.** ProjectTree compare-and-replace targets a caller-specified byte-aligned, codepoint-boundary source selection, verifies the expected old string, enforces the real file capacity and commits exactly one new disk generation; stale file and process generations are rejected.
3. **Authorized desktop workflow.** New routes for source hygiene and compare-and-replace require FILE_WRITE, valid PID generation and unexpired editor session before touching the editor or the mounted project.
4. **Non-submitting shell argument fix.** Live terminal command editor replaces one whitespace-delimited argument in-place, preserving other case-sensitive arguments, exact spacing, history and clipboard; validates printable, non-space ASCII and full input bounds first.
5. **Numbered, bounded source inspection.** The new `sourceview /src 2 3` terminal command displays exactly three one-based numbered UTF-8 source rows from the mounted COW filesystem, staging complete output before returning it. Invalid rows, expired access and short buffers publish no bytes.

## Tests and implementation guarantees

Five existing production Rust modules plus this documentation; no synthetic per-agent files. Native unit tests cover Unicode selections, realistic VFS save and remount, undo/redo, exact old-data checks, expired/recycled identities, invalid bytes, missing rows and short output. No external dependency or heap allocation was added.

## Release gates

One consolidated source commit and one PR, DRAFT until all mandatory CI and real QEMU boot checks on the final exact PR head are green. Recover failed jobs by reading actual logs and fixing implementation, never disabling tests. Preserve one commit via amend/re-squash; merge only when repository protection allows it.

Five engineering workstreams were implemented directly through the connected GitHub repository; five distinct autonomous Codex worker processes were not launched.
