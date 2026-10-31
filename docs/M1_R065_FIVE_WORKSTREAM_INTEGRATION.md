# IndianOS M1/R065 — five implemented workstation workstreams

Starting main: `f3ba4daad9378e8258cfdaf87b54e3e8b1f29989` (merged PR #686).

Five independently scoped *functional workstreams* are integrated through the
GitHub-connected coordinator in ONE source commit, ONE PR.
**Actual independent parallel Codex coding workers launched: 0.**
This interface cannot spawn separate Codex worktree agents.

## New production functionality

1. **RAM Files byte-range splice:** `RamFs::splice_if_file_revision`
   performs revision-checked insert/delete/replacement of arbitrary binary
   ranges, without sparse holes or partial writes, using a fixed-size scratch
   buffer. No-op preserves revisions. RAM state is not reboot-durable.
   Three Rust regression tests.
2. **Durable developer editor:** `DiskEditor::comment_source_range`
   comments/uncomments validated UTF-8 source rows in one undoable edit,
   preserving blanks and newline boundaries. Source is persisted only by
   the existing authenticated COW save; real remount regression included.
   Three Rust regression tests.
3. **Developer desktop:** `SurfaceLayout::tile_master_stack` lays out
   a primary editor and secondary panes with a configurable left-side
   percentage, exact odd-pixel distribution, owner verification, atomic
   geometry and redraw epochs. Three Rust regression tests.
4. **Interactive Terminal history:** `InputLine::forget_history_command`
   removes exact private command history entries, compacting the fixed
   ring and restoring unsent drafts if a history entry was being browsed.
   Does not execute commands. Three Rust regression tests.
5. **Persistent binary Terminal:** `patchhex /file offset HH` changes
   one existing binary byte using checked COW overwrite with READ+WRITE
   authentication, protected namespace guards, short-output preflight,
   and remount validation. No-op leaves disk generation unchanged.
   Three Rust regression tests.

**15 new Rust regression tests** across 5 existing production modules,
plus this document. No agent placeholders or disabled checks.

This batch implements backing services and tests; it is NOT evidence that
graphical IDE, window compositor and kernel runtime acceptance work in the
real guest until actual QEMU tests verify that behavior.

## Merge gates

- Compile and run all Rust host unit tests on exact PR head.
- Compile no_std freestanding kernel and run combined M1 regression.
- Security S0 authorization and protected filesystem paths.
- Kernel ELF/ISO boot and QEMU ring-3, Desktop/Terminal/Files and durable
  reboot acceptance on exact PR head.
- Independent review and repository mandatory checks.
- Do not touch physical disks. Keep DRAFT and do not merge when checks
  fail, are skipped, or are pending.

For any regression failure, fix the real root cause on the SAME integration
branch and re-squash back to one source commit. Report verified CI only.
