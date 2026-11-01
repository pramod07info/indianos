# IndianOS M1/R081 — 30 real workstation tasks in 5 workstreams

Starting main: \`1f7c20ec2411b4c5430f04fa8416bcf795ab1cac\` after merged R080 (#722).
Policy: **30 production tasks, five explicit subsystem workstreams,
one integrated source commit, and one PR**. The workstreams represent
ownership and integration boundaries, not independently running agents.

## Workstream 1 — Active source-editor buffers (6 tasks)

01. \`EditorPool::text_metrics\` — Unicode source bytes, lines, scalars,
    optimistic FNV digest and dirty state on the unsaved live document.
02. \`EditorPool::find_across_slots\` — literal Unicode search in both
    authenticated open documents, with starting slot/cursor.
03. \`EditorPool::replace_selection_if_equal\` — optimistic in-memory
    exact-selection edit, preserving undo state on stale rejection.
04. \`EditorPool::insert_line_from_slot\` — copy a complete UTF-8 source
    row between two open documents as one undoable target edit.
05. \`EditorPool::mirror_slot_if_checksum\` — replace an open target buffer
    with another slot only when its visible revision matches.
06. \`EditorPool::undo_all_ready\` — preflight every open buffer's identity
    and undo snapshot before undoing all active slots.

## Workstream 2 — Real persistent project source files (6 tasks)

07. \`ProjectTree::source_slice_checksum\` — bounded Unicode slice digest.
08. \`ProjectTree::read_source_window_if_checksum\` — revision-checked
    UTF-8 window copy with atomic caller-buffer preflight.
09. \`ProjectTree::patch_source_if_checksum\` — UTF-8 source splice in a
    single copy-on-write disk commit.
10. \`ProjectTree::replace_first_literal_if_checksum\` — source search
    and optimistic first-hit durable substitution.
11. \`ProjectTree::prepend_source_if_checksum\` — guarded header insertion
    without lost-update writes.
12. \`ProjectTree::erase_source_range_if_checksum\` — Unicode range
    deletion with the exact observed file revision.

## Workstream 3 — Durable terminal commands (6 tasks)

13. \`castext /file FNV OFFSET COUNT TEXT\` — conditional Unicode patch.
14. \`casinsert /file FNV OFFSET TEXT\` — conditional Unicode insertion.
15. \`caserase /file FNV OFFSET COUNT\` — conditional Unicode range erasure.
16. \`unicodehead /file SCALARS\` — first complete Unicode code points.
17. \`unicodetail /file SCALARS\` — last complete Unicode code points.
18. \`textverify /file\` — verified disk file reports \`utf8\` or \`binary\`.
All three mutations share the bounded, no-partial-output
\`conditional_text_patch\` COW transaction helper.

## Workstream 4 — Interactive terminal command drafts (6 tasks)

19. \`InputLine::argument_count\` — full live command token count.
20. \`InputLine::find_argument_exact\` — case-sensitive argument lookup.
21. \`InputLine::uppercase_argument\` — one-token ASCII uppercase.
22. \`InputLine::lowercase_argument\` — one-token ASCII lowercase.
23. \`InputLine::replace_argument_if_equal\` — stale-protected token edit.
24. \`InputLine::rotate_arguments_left\` — rotate tokens after the verb
    in one edit while preserving the command verb.

## Workstream 5 — Authenticated persistent desktop APIs (6 tasks)

25. \`Desktop::project_source_slice_checksum\` — FILE_READ-gated lookup.
26. \`Desktop::project_read_source_window_if_checksum\` — FILE_READ-gated
    revision-matched window read.
27. \`Desktop::project_patch_source_if_checksum\` — FILE_WRITE-gated edit.
28. \`Desktop::project_replace_first_literal_if_checksum\` — FILE_WRITE
    guarded source substitution.
29. \`Desktop::project_prepend_source_if_checksum\` — FILE_WRITE header edit.
30. \`Desktop::project_erase_source_range_if_checksum\` — FILE_WRITE
    checked range removal.

## Verification and limits

- Production implementation is in **five existing Rust modules**, not
  empty \`agent_XX.rs\` files; every stream has targeted regression tests.
- Host tests exercise UTF-8 edge cases, stale updates, permission expiry,
  recycled process identity and actual \`HierVfs\` cold remount.
- FNV64 is **not a cryptographic signature**; it is an optimistic
  source-revision conflict hint.
- The fixed-bank volume still bounds file/object capacity.
- This is source editor, project, terminal and desktop **service-layer**
  functionality, not proof of a fully graphical ring-3 IDE or completed
  independent kernel hardware support.
- **Merge gate:** verify all required GitHub Actions checks including
  combined Rust regression, durable desktop, adversarial security,
  kernel ELF/ISO serial boot and real QEMU runtime. Do not merge on pending
  or failed checks; correct actual failures and amend single source commit.
- Five agent workstations means five logical workstreams in this execution;
  no independent coding agent instances were spawned.

Status is determined by GitHub Actions only; committing code is not CI proof.
