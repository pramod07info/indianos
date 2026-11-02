# IndianOS R093 — 14 hierarchical Files/VFS integrations

This batch extends **real runtime-facing `HierVfs` Rust code**, not merely the lower-level COW storage API. It validates the existing hierarchical path, parent directory and conflicting file/directory aliases before forwarding to the R092 transactional operation. Each bounded user transfer is <=256 bytes. There are no new privileged raw block writes.

## Implemented integration tasks

| ID | HierVfs operation | DurableFs backend |
|---|---|---|
| R093-01 | `append_file` | `append_path` |
| R093-02 | `fill_file_range` | `fill_range_path` |
| R093-03 | `insert_file_range` | `insert_range_path` |
| R093-04 | `remove_file_range` | `remove_range_path` |
| R093-05 | `splice_file` | `splice_path` |
| R093-06 | `swap_file_names` | `swap_paths` |
| R093-07 | `rename_file_overwrite` | `rename_overwrite_path` |
| R093-08 | `compare_exchange_file` | `compare_exchange_path` |
| R093-09 | `unlink_if_equal` | `delete_if_equal_path` |
| R093-10 | `rename_file_if_equal` | `rename_if_equal_path` |
| R093-11 | `copy_file_range` | `copy_range_into_path` |
| R093-12 | `replace_all_file` | `replace_all_path` |
| R093-13 | `crc32_file` | `verified_crc_path` |
| R093-14 | `file_equals_bytes` | `equals_path` |

The internal validation helper `r093_checked_file` is not counted as a feature. R093 adds 9 new Rust integration tests: reboot/remount, extension, splice, atomic rename, optimistic edits, conditional mutations, duplicate filename protections and bounded request failure.

**Scope accounting:** 90 requested additional production implementation tasks. R092 implemented 17 distinct DurableFs operations; R093 implements 14 separate HierVfs integrations = **31 completed coding work items**, pending all required CI checks for the exact final SHA. **59 additional tasks not yet implemented.** R093 integrations are not themselves a new user-space syscall: existing ring3 IPC, application commands, process identity and QEMU boot gates still need work. No high-level M1 gate claimed closed.
