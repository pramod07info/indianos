# IndianOS M1/R079: 20 workstation functions / five integration scopes

Parent commit: 83576ad01f8edf4ee1cc9024d6b67ab9cc4b70ee (merged R078).
Policy: **one source commit, one pull request, no placeholder agent files**.
Five workstreams represent subsystem ownership, not independently running agents.

## Workstream A — multi-document editor (tasks 01–04)

01. Authenticated per-slot literal Unicode replace-all, one undoable buffer edit.
02. Authenticated per-slot search backward by byte cursor.
03. Authenticated paginated source matches with resumable byte offsets.
04. Authenticated Unicode scalar/line position from UTF-8 byte offset.

## Workstream B — durable project files (tasks 05–08)

05. Read source summary: byte length, real source lines, Unicode scalars and FNV.
06. Compare actual on-disk UTF-8 sources for first differing byte.
07. Optimistically replace source only if its observed checksum still matches.
08. Optimistically delete source only if its observed checksum still matches.

## Workstream C — real persistent terminal (tasks 09–12)

09. \`castruncate /file OLD_BYTES NEW_BYTES\`: optimistic COW resizing.
10. \`casclone /src /dst EXPECTED_FNV64\`: revision-checked new-file clone.
11. \`wordpage /file CURSOR LIMIT\`: paginated UTF-8 word start byte offsets.
12. \`idcount /file IDENTIFIER\`: count whole identifier matches.

## Workstream D — live terminal input (tasks 13–16)

13. Uppercase only the ASCII word at the active caret.
14. Lowercase only the ASCII word at the active caret.
15. Duplicate a caret word as \`name_name\` within the bounded draft.
16. Surround a caret word with caller-selected printable delimiters.

## Workstream E — authenticated project desktop routes (tasks 17–20)

17. Read-only, authorization-gated source summary.
18. Read-only, authorization-gated full-source comparison.
19. FILE_WRITE-gated conditional source replacement on mounted disk.
20. FILE_WRITE-gated conditional source deletion on mounted disk.

## Evidence and limits

- All production changes are in five existing Rust subsystem modules.
- Tests exercise real \`HierVfs\` COW disk remount, Unicode boundaries,
  optimistic conflict handling, malformed requests and denied identity.
- Small existing volume (eight-object backing store) remains a known limit.
- No fabricated QEMU boot or graphical IDE claims; backend APIs remain
  separate from actual compositor/ring-3 visual integration.
- FNV hashes are noncryptographic revision hints, not authenticators.
- All required CI checks (kernel build, QEMU, regression, security) must
  independently confirm success before PR merge.
- On CI failure, correct code/tests without disabling coverage and amend
  the original feature commit rather than adding extra source commits.

Status: submitted for CI, not verified merely by code creation.
