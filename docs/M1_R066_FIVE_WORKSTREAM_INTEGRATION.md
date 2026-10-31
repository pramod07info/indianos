# IndianOS M1/R066 — five production workstation features, one commit

Starting merged main: `5b3610ad177613f2d85d178d1f5248b00bc4a6b1` (PR #687, R065).

One connected GitHub coordinator implemented five distinct subsystem workstreams.
**Independent Codex worker sessions actually launched: 0**, because this
connector does not provision five separate coding worktrees.
Do not count PRs or host APIs as proven graphical user functionality.

## Real implementation

1. RAM Files: `RamFs::splice_batch_if_versions` atomically combines multiple
   per-file-revision-checked binary splices on a private bounded shadow RAM
   filesystem. All late invalid offsets, bad paths, duplicate targets and
   capacity failures reject without publishing earlier changes. 3 tests.
2. Disk editor: `DiskEditor::replace_source_lines` atomically replaces an
   inclusive Unicode source row selection with multi-row UTF-8 text, preserving
   newline boundaries, one undo/redo operation and actual COW save/remount.
   Protected by authenticated caller identity and size validation. 3 tests.
3. Developer desktop: `SurfaceLayout::rotate_panels` rotates authenticated
   existing panel geometry while preserving ownership. Fully validates every
   window before one geometry/epoch transaction; wrapped no-ops do not
   consume redraw epochs. 3 tests.
4. Interactive Terminal: `InputLine::paste_history_fragment` inserts a
   bounded selected substring of an actual successfully submitted historical
   command at the current caret, without execution or clipboard mutation.
   Invalid source/selection or full drafts remain unchanged. 3 tests.
5. Persistent Terminal: `patchrange /path offset HEXPAIRS` decodes a bounded
   multi-byte binary patch, validates READ+WRITE capability, reserved paths,
   output capacity, existing file bounds, then applies one checksummed COW
   overwrite. No-op does not advance disk generation; tested across block
   device remount. 3 tests.

**15 new Rust regression tests**, five existing Rust production modules,
one documentation file. No disabled checks or synthetic agent files.

## Required PR gates

- Exact PR-head Rust host compilation and combined regression tests.
- Freestanding no_std kernel build, ELF/ISO and QEMU boot.
- Security S0, authenticated file permissions and failure-atomicity tests.
- Real QEMU ring-3 graphical Files/Terminal, persistence and reboot acceptance.
- Independent review and required branch checks.
- No physical disk formatting.
- Keep the PR draft and unmerged unless every mandatory exact-head check
  succeeds. For CI failures, amend or re-squash fixes into one source commit
  on the same branch and rerun.
