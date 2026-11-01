# IndianOS M1/R082 — 50 real workstation operations, five scoped workstreams

Base: merged R081 main b3a3014693a7d3ed374a5513cbb4ccd1e8208986

Integration policy: one source commit and one PR; merge only after every mandatory GitHub CI job succeeds.

Five workstreams are subsystem ownership scopes, not five independently spawned coding agents.

## Completed implementation checklist

### Editor — ten operations

- [x] 01. `count_literal`
- [x] 02. `last_literal`
- [x] 03. `read_source_row`
- [x] 04. `append_source_text`
- [x] 05. `prepend_source_text`
- [x] 06. `replace_last_literal`
- [x] 07. `ensure_final_newline`
- [x] 08. `remove_final_newline`
- [x] 09. `replace_all_at_revision`
- [x] 10. `redo_all_ready`

### Project — ten operations

- [x] 11. `count_source_literal`
- [x] 12. `last_source_literal`
- [x] 13. `read_source_row_bytes`
- [x] 14. `count_nonblank_source_rows`
- [x] 15. `source_starts_with`
- [x] 16. `source_ends_with`
- [x] 17. `replace_last_at_revision`
- [x] 18. `ensure_source_newline_at_revision`
- [x] 19. `remove_source_newline_at_revision`
- [x] 20. `replace_source_row_at_revision`

### Terminal — ten operations

- [x] 21. `casprepend`
- [x] 22. `casstrip`
- [x] 23. `casupper`
- [x] 24. `caslower`
- [x] 25. `casnewline`
- [x] 26. `unicodepage`
- [x] 27. `tabpositions`
- [x] 28. `crpositions`
- [x] 29. `leadingspaces`
- [x] 30. `sourceprefix`

### Input — ten operations

- [x] 31. `read_argument`
- [x] 32. `argument_equals`
- [x] 33. `argument_has_prefix`
- [x] 34. `argument_has_suffix`
- [x] 35. `prefix_argument`
- [x] 36. `suffix_argument`
- [x] 37. `capitalize_argument`
- [x] 38. `reverse_argument`
- [x] 39. `rotate_arguments_right`
- [x] 40. `delete_argument_if_equal`

### Desktop — ten operations

- [x] 41. `project_count_source_literal`
- [x] 42. `project_last_source_literal`
- [x] 43. `project_read_source_row_bytes`
- [x] 44. `project_count_nonblank_source_rows`
- [x] 45. `project_source_starts_with`
- [x] 46. `project_source_ends_with`
- [x] 47. `project_replace_last_at_revision`
- [x] 48. `project_ensure_source_newline_at_revision`
- [x] 49. `project_remove_source_newline_at_revision`
- [x] 50. `project_replace_source_row_at_revision`

## Evidence and limits

- Five existing Rust production modules changed; tests in the corresponding modules exercise actual data paths and failure cases.
- Editor changes remain unsaved until explicit save_all; project/Terminal changes use the mounted CRC-protected copy-on-write VFS.
- UTF-8 validation, bounded outputs, live process ownership and expired capabilities reject unsafe operations.
- Durable project mutations use actual on-disk bytes and optimistic revision validation.
- FNV64 is a noncryptographic optimistic revision hint; not a signature or authorization substitute.
- The current fixed-capacity filesystem still limits the number and size of files.
- This batch does not claim the graphical IDE is completed or independently launched 5 agent processes.
- Run combined Rust regression, durable desktop/Files, no_std security checks, QEMU runtime, kernel ELF/ISO serial boot.
- Fix failing production code or incorrect tests without suppressing failures; amend existing single source commit if needed.
- Never merge until all required checks pass and branch protection permits merging.
