# IndianOS M1 / R083: 100 meaningful source services across five workstreams

Base: verified R082 merged main 884867dfaa6af85199e07d1a56d1932063407bde.
Scope: 5 ownership workstreams × 20 Rust production operations = 100. One source commit, one PR.
These workstreams were implemented in a single coordinated coding session; five independent agent sessions were NOT launched.
100 operations include 20 native desktop API routes; the routes are not separate GUI features.

## Detailed checklist

### A: Unsaved editor

- [x] 001 `buffer_contains_literal`
- [x] 002 `buffer_starts_with`
- [x] 003 `buffer_ends_with`
- [x] 004 `buffer_row_count`
- [x] 005 `buffer_lines_with`
- [x] 006 `buffer_first_line_with`
- [x] 007 `buffer_last_line_with`
- [x] 008 `buffer_tab_count`
- [x] 009 `buffer_crlf_count`
- [x] 010 `buffer_trailing_spaces`
- [x] 011 `buffer_replace_prefix`
- [x] 012 `buffer_replace_suffix`
- [x] 013 `buffer_erase_first_literal`
- [x] 014 `buffer_erase_last_literal`
- [x] 015 `buffer_insert_before_first`
- [x] 016 `buffer_insert_after_last`
- [x] 017 `buffer_uppercase_ascii`
- [x] 018 `buffer_lowercase_ascii`
- [x] 019 `buffer_trim_trailing_spaces`
- [x] 020 `buffer_normalize_crlf`

### B: Persistent project

- [x] 021 `source_contains_literal`
- [x] 022 `source_line_count_matching`
- [x] 023 `source_first_matching_line`
- [x] 024 `source_last_matching_line`
- [x] 025 `source_unicode_scalars`
- [x] 026 `source_tab_characters`
- [x] 027 `source_crlf_pairs`
- [x] 028 `source_ascii_digits`
- [x] 029 `source_trailing_spaces`
- [x] 030 `source_longest_row_bytes`
- [x] 031 `replace_source_prefix_at_revision`
- [x] 032 `replace_source_suffix_at_revision`
- [x] 033 `erase_first_literal_at_revision`
- [x] 034 `erase_last_literal_at_revision`
- [x] 035 `insert_before_first_at_revision`
- [x] 036 `insert_after_last_at_revision`
- [x] 037 `uppercase_source_ascii_at_revision`
- [x] 038 `lowercase_source_ascii_at_revision`
- [x] 039 `trim_source_trailing_spaces_at_revision`
- [x] 040 `normalize_source_crlf_at_revision`

### C: Durable Terminal

- [x] 041 `srcupper`
- [x] 042 `srclower`
- [x] 043 `srcdigit`
- [x] 044 `srcpunct`
- [x] 045 `srcnonascii`
- [x] 046 `srcwords`
- [x] 047 `srctokens`
- [x] 048 `srctabs`
- [x] 049 `srccrlf`
- [x] 050 `srclongest`
- [x] 051 `srcshortest`
- [x] 052 `srctrailws`
- [x] 053 `srcleadingws`
- [x] 054 `srcblank`
- [x] 055 `srcunique`
- [x] 056 `srcmaxcol`
- [x] 057 `srcmixednl`
- [x] 058 `srchasbom`
- [x] 059 `srcfinalnl`
- [x] 060 `srcnul`

### D: Live keyboard input

- [x] 061 `argument_length`
- [x] 062 `argument_contains`
- [x] 063 `argument_is_ascii`
- [x] 064 `argument_is_number`
- [x] 065 `argument_is_option`
- [x] 066 `argument_is_absolute_path`
- [x] 067 `argument_is_lowercase_ascii`
- [x] 068 `argument_is_uppercase_ascii`
- [x] 069 `first_argument_prefix`
- [x] 070 `count_option_arguments`
- [x] 071 `remove_argument_prefix`
- [x] 072 `remove_argument_suffix`
- [x] 073 `keep_argument_first`
- [x] 074 `drop_argument_first`
- [x] 075 `swap_argument_ascii_case`
- [x] 076 `replace_argument_first_byte`
- [x] 077 `replace_argument_last_byte`
- [x] 078 `strip_argument_dashes`
- [x] 079 `ensure_argument_dash`
- [x] 080 `repeat_argument_token`

### E: Desktop authorization

- [x] 081 `project_source_contains_literal`
- [x] 082 `project_source_line_count_matching`
- [x] 083 `project_source_first_matching_line`
- [x] 084 `project_source_last_matching_line`
- [x] 085 `project_source_unicode_scalars`
- [x] 086 `project_source_tab_characters`
- [x] 087 `project_source_crlf_pairs`
- [x] 088 `project_source_ascii_digits`
- [x] 089 `project_source_trailing_spaces`
- [x] 090 `project_source_longest_row_bytes`
- [x] 091 `project_replace_source_prefix_at_revision`
- [x] 092 `project_replace_source_suffix_at_revision`
- [x] 093 `project_erase_first_literal_at_revision`
- [x] 094 `project_erase_last_literal_at_revision`
- [x] 095 `project_insert_before_first_at_revision`
- [x] 096 `project_insert_after_last_at_revision`
- [x] 097 `project_uppercase_source_ascii_at_revision`
- [x] 098 `project_lowercase_source_ascii_at_revision`
- [x] 099 `project_trim_source_trailing_spaces_at_revision`
- [x] 100 `project_normalize_source_crlf_at_revision`

## Implementation and validation contract

- Substantive edits live in five existing Rust kernel-core modules; no placeholder agent files.
- Editor methods operate on unsaved owner-bound UTF-8 text and preserve undo semantics, with explicit save required.
- Project source edits check current mounted COW filesystem generation and optimistic FNV64 revision before the write.
- Desktop project routes require live process identity and FILE_READ / FILE_WRITE grant checks.
- Terminal adds 20 actual read-only disk-backed Unicode/ASCII diagnostics, with no device formatting or raw block access.
- InputLine edits modify active, unsent shell command token bytes atomically within MAX_LINE, without implicit execution.
- Tests include real HierVfs cold remount, stale identity/revisions, output preflight, Unicode and terminal registry unique lowercase naming.
- FNV64 is NOT cryptographic authentication; the small fixed-capacity VFS remains a limitation.
- This is not a completed graphical IDE or proof of newly implemented hardware drivers.
- Merge ONLY when combined-regression, durable-files-desktop, adversarial security, runtime-qemu, ELF/ISO serial boot, SHA, review and coordination checks all pass.
- On failures fetch the actual logs, repair production/tests appropriately, amend the same one source commit, and reverify.
