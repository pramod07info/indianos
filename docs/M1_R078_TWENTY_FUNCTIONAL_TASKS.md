# IndianOS M1/R078 — 20 actual workstation tasks in five logical workstreams

Parent main: 2123d7a1d1a1de738bb5afcd6b5e23c5730352ad
Delivery policy: exactly **one** source commit, **one** integration PR.

These five workstreams are independent subsystem scopes, **not five
simultaneously launched coding agents**. Implementation must be validated by
GitHub Actions before merging. No generated agent_XX.rs placeholder modules.

## Workstream 1: Disk-backed source editor (4)
01. Owner/slot-scoped literal find in an open document.
02. Unicode-safe source selection copy from an owned document.
03. Undoable Unicode source cut in one owned document.
04. Unicode source paste/replacement in one owned document.

## Workstream 2: Persistent project service (4)
05. Complete-line UTF-8 source paging with a continuation cursor.
06. Whole-identifier search across checked UTF-8 project files, paged.
07. Conditional source file copy guarded by observed source checksum.
08. Conditional source rename guarded by observed source checksum.

## Workstream 3: Durable Terminal commands (4)
09. \`textpage /file BYTE_OFFSET COUNT\` returns a UTF-8-safe file page.
10. \`idpage /file IDENTIFIER MATCH_CURSOR COUNT\` returns whole-token offsets.
11. \`caswrite /file EXPECTED_FNV64 NEW_TEXT\` performs checked COW replacement.
12. \`casappend /file EXPECTED_BYTE_LENGTH TEXT\` performs checked COW append.

## Workstream 4: Interactive Terminal editing (4)
13. Calculate the ASCII identifier range at the current caret.
14. Copy the caret identifier to the private session clipboard.
15. Cut the caret identifier while preserving adjacent argument bytes.
16. Replace the caret identifier atomically without submitting command.

## Workstream 5: Authenticated desktop service routes (4)
17. Find source text within a specific owned editor document.
18. Copy UTF-8 source selections only with an active FILE_READ grant.
19. Cut source selections only with active FILE_WRITE grant.
20. Paste Unicode source text only with active FILE_WRITE grant.

## Safety and acceptance

- Existing mounted checksum-protected COW volume is the only persistence.
- Reject stale owner/generation, malformed UTF-8, incorrect boundaries,
  buffer exhaustion, protected Terminal paths and expired Desktop grants.
- Check authorization before reading protected editor buffers.
- No code executes merely by editing a command line.
- The existing VFS supports only eight total persistent objects.
- FNV is a collision-prone *optimistic conflict detector*, never a trust,
  identity or executable verification mechanism.
- All new functionality must have tests for success/failure and real remounts.
- Kernel boot, freestanding compile, security, desktop and QEMU CI must pass.
- The GUI renderer and true ring-3/editor integration remain separate;
  this batch adds service and live line-editor functionality, **not a full IDE**.
- Any CI failure requires a production/test correction before merge. Amend
  the integration source commit, rather than adding one commit per small fix.

Validation status: **unverified until GitHub Actions reports results**.
