# IndianOS M1/R080 — 20 substantial durable workstation tasks

Base: merged main \`66863c5157ddcc5aab5ccfed0f6616ed465de66b\`
Integration: **one source commit, one PR**. Workstreams represent module ownership,
not five independently launched coding agents.

## Stream A — active disk editor / multi-document reconciliation

1. Compare two authenticated unsaved Unicode editor buffers and report the first differing byte.
2. Transfer selected UTF-8 text from one open document into another with one undoable edit.
3. Refresh just one editor slot from disk with explicit dirty-discard consent.
4. Close an entire editor session atomically after preflighting ownership and dirty buffers.

## Stream B — persistent project files

5. Read a complete UTF-8 source snapshot only if its checksum revision matches.
6. Append a Unicode suffix to a source with an optimistic checksum guard.
7. Truncate to a validated UTF-8 boundary with optimistic checksum guard.
8. Extract an exact UTF-8 slice to a new COW-backed project file, gated by checksum.

## Stream C — persistent terminal and filesystem safety

9. \`casrm /file FNV64\` deletes a file only at its observed checksum revision.
10. \`casmv /src /dst FNV64\` safely renames a verified source version.
11. \`linepage /file START COUNT\` returns complete Unicode lines by page.
12. \`textstats /file\` reports disk byte count, source row count and Unicode scalars.
**Additional regression repair:** \`copyrange\` preflights output capacity
before creating the new file, so a short output cannot publish metadata.

## Stream D — interactive terminal draft

13. Copy a selected shell-style command argument to the private clipboard.
14. Cut a selected argument, retaining token in clipboard, without submitting it.
15. Wrap an argument with printable caller-supplied delimiters atomically.
16. Swap adjacent arguments while preserving their exact spaces and case.

## Stream E — permission-gated desktop APIs

17. Route active document-to-document comparison with \`FILE_READ\` checks.
18. Route Unicode document selection transfer with \`FILE_READ | FILE_WRITE\`.
19. Route explicit one-slot refresh with \`FILE_READ | FILE_WRITE\`.
20. Route all-slots close with \`FILE_WRITE\` and all-or-nothing dirty checks.

## Acceptance and limitations

- Test on actual CRC-protected copy-on-write HierVfs disk and cold remount.
- Reject stale owners/process generations, invalid Unicode byte boundaries,
  blocked paths, expired privileges, existing destination names, and undersized
  caller buffers; never claim output/file atomicity without verification.
- FNV-64 is a noncryptographic optimistic conflict hint, not an authenticator.
- The backing HierVfs still has a fixed small object count.
- This batch extends real Rust disk-backed developer services, not a fully
  graphical or ring-3 source IDE; independent kernel/device progress remains.
- No placeholder \`agent_nn.rs\` files. CI must confirm regression, security,
  kernel serial boot/ISO and QEMU runtime before merge.
- If CI fails, fix production/tests without disabling checks and amend the same
  one integration commit.

CI evidence must be read from GitHub; code creation itself is not test success.
