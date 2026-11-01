# IndianOS M1/R075 — 20 substantial functions in 5 workstation workstreams

Base: merged R074 plus verified QEMU screenshot history; main SHA `1380f8e64ca6a4adf9f7030728d5d80abb7755c6`.

## 5 workstreams × 4 implemented features = 20

### A. Rust DiskEditor / source editing (4)
1. `reverse_source_lines`: select and reverse up to 128 source rows as one undoable, unsaved UTF-8 editor mutation.
2. `remove_source_blank_rows`: delete whitespace-only source rows as one reversible edit.
3. `convert_source_lf_to_crlf`: atomically convert LF-only Unicode source to CRLF for Windows compatibility. Reject mixed/bare CR instead of silently altering it.
4. `deduplicate_adjacent_source_rows`: remove only consecutive identical source lines; all other content stays in order.

### B. ProjectTree / durable source management (4)
5. `apply_source_patches_if_equal`: compare and apply up to eight nonoverlapping Unicode source patches in one durable COW generation.
6. `measure_source_line`: read actual stored source row bytes, Unicode scalar count and leading/trailing whitespace counts; never modify filesystem generation.
7. `append_source_if_suffix_equal`: append a source fragment only if the observed UTF-8 suffix is still present, rejecting stale reads without a write.
8. `extract_source_selection`: create a new durable project file from an exact Unicode-boundary source range; never overwrite an existing destination.

### C. Desktop / authorized service integration (4)
9. `editor_reverse_source_lines`: route multirow editor operations through live FILE_WRITE authorization.
10. `project_measure_source_line`: authenticated FILE_READ diagnostics for the mounted project tree.
11. `project_append_source_if_suffix_equal`: FILE_WRITE-gated safe optimistic append from developer workspace.
12. `project_extract_source_selection`: require both read and write permissions to create source selections as new project files.

### D. Interactive shell / editor (4)
13. `duplicate_argument`: clone a selected unsent command token without touching clipboard/history or executing it.
14. `compact_argument_spacing`: collapse redundant argument separator spaces and trim surrounding spaces without changing case-sensitive values.
15. `insert_argument_before`: insert one printable token at a specified argument position or append at the end with capacity checks.
16. `move_argument_to_end`: reorder an existing token to the final position, reconstructing one bounded draft transaction.

### E. Persistent terminal / actual source inspection (4)
17. `textprofile /src`: UTF-8 scalar count, bytes, source rows, and maximum scalar values per row.
18. `blankrows /src`: 1-based row numbers containing only ASCII indentation or no characters.
19. `indentmap /src`: one-based per-row leading space and leading TAB counts, without altering source.
20. `eolcheck /src`: LF, CRLF, lone CR and final-newline report, preserving the file byte-for-byte.

## Engineering requirements and limitations

All code extends five **existing Rust production modules**; this document is the sixth changed file. Each workstream includes regression tests for Unicode, real VFS mount/reboot, undo/redo or read-only behavior, capacity, stale PID generation, grant expiry and no partial writes. These are **20 distinct functions**, not 20 new kernel subsystems. The existing authenticated desktop services are still not fully wired into a graphical IDE; the file system remains bounded.

**One consolidated source commit, one existing PR #708**. This PR remains draft until exact-head Rust tests, no_std build, S0 security, actual QEMU boot, real durable desktop and independent review pass. Correct defects on this integration branch, re-squash if needed, never suppress failing tests. Merge only after required checks and repository protections are green. No physical disk I/O. Five engineering workstreams were performed through the connected GitHub tool, **not** by five independent autonomous Codex runtimes.
