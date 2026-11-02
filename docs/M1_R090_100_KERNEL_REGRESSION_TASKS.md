# IndianOS R090 — 100 Executable Kernel Regression Tasks

## Scope and status
- **100 distinct Rust `#[test]` functions** have been written, exactly 20 for each of five owned kernel areas, all in existing production Rust modules.
- These are **100 acceptance/fault-injection regression tasks**, **not 100 new operating-system features** or eight completed high-level M1 gates.
- The R089 production changes (block geometry, partition catalog, durable format, Terminal input, ATA settle) remain part of the *same consolidated one-commit PR*. This expansion adds executable tests rather than 100 checklist placeholders.
- Implemented source does not equal *tested and passed*: status is **awaiting exact amended-commit CI result** until GitHub confirms every required workflow. No local execution or separate five agents is claimed.
- Tests do not touch physical disks. Host Rust, no_std/security, QEMU disposable-disk and live ring3 acceptance remain governed by [M1 issue #616](https://github.com/pramod07info/indianos/issues/616).
- All tasks have precise Rust functions named `r090_<group><ordinal>_<task_name>`. A failing case is **not** accepted until fixed. All original and new tests must remain enabled.

## Exactly 100 implemented, unverified regression tasks

### B — Block geometry and bounded I/O (20)
1. `r090_b01_reject_empty_multisector_read` — reject empty multisector read.
2. `r090_b02_reject_partial_sector_read` — reject partial sector read.
3. `r090_b03_reject_empty_multisector_write` — reject empty multisector write.
4. `r090_b04_reject_partial_sector_write` — reject partial sector write.
5. `r090_b05_read_last_sector_at_exact_boundary` — read last sector at exact boundary.
6. `r090_b06_write_last_sector_at_exact_boundary` — write last sector at exact boundary.
7. `r090_b07_read_overrun_fails_before_publishing` — read overrun fails before publishing.
8. `r090_b08_write_overrun_fails_without_partial_sector` — write overrun fails without partial sector.
9. `r090_b09_three_sector_roundtrip_covers_middle` — three sector roundtrip covers middle.
10. `r090_b10_midtransfer_failure_scrubs_all_readback` — midtransfer failure scrubs all readback.
11. `r090_b11_first_sector_failure_scrubs_all_readback` — first sector failure scrubs all readback.
12. `r090_b12_window_exposes_bounded_sector_count` — window exposes bounded sector count.
13. `r090_b13_window_write_does_not_touch_adjacent_sector` — window write does not touch adjacent sector.
14. `r090_b14_window_read_uses_physical_start_offset` — window read uses physical start offset.
15. `r090_b15_window_rejects_outside_read_without_driver_access` — window rejects outside read without driver access.
16. `r090_b16_window_rejects_outside_write_without_driver_access` — window rejects outside write without driver access.
17. `r090_b17_readonly_wrapper_denies_device_writes` — readonly wrapper denies device writes.
18. `r090_b18_readonly_wrapper_denies_flush` — readonly wrapper denies flush.
19. `r090_b19_readonly_failed_driver_does_not_taint_caller` — readonly failed driver does not taint caller.
20. `r090_b20_failed_flush_retires_window_permanently` — failed flush retires window permanently.

### P — MBR/GPT partition validation (20)
21. `r090_p01_mbr_rejects_missing_first_signature_byte` — mbr rejects missing first signature byte.
22. `r090_p02_mbr_rejects_missing_second_signature_byte` — mbr rejects missing second signature byte.
23. `r090_p03_mbr_rejects_invalid_boot_indicator` — mbr rejects invalid boot indicator.
24. `r090_p04_mbr_rejects_start_zero_for_live_partition` — mbr rejects start zero for live partition.
25. `r090_p05_mbr_rejects_live_partition_zero_sectors` — mbr rejects live partition zero sectors.
26. `r090_p06_mbr_rejects_partition_larger_than_disk` — mbr rejects partition larger than disk.
27. `r090_p07_mbr_rejects_overlapping_second_partition` — mbr rejects overlapping second partition.
28. `r090_p08_mbr_supports_noncontiguous_partition_ordinal` — mbr supports noncontiguous partition ordinal.
29. `r090_p09_mbr_rejects_inactive_empty_slot_marked_bootable` — mbr rejects inactive empty slot marked bootable.
30. `r090_p10_mbr_rejects_empty_slot_nonzero_chs` — mbr rejects empty slot nonzero chs.
31. `r090_p11_mbr_rejects_extended_partition_not_implemented` — mbr rejects extended partition not implemented.
32. `r090_p12_mbr_missing_requested_ordinal_does_not_write` — mbr missing requested ordinal does not write.
33. `r090_p13_mbr_copy_to_short_output_remains_empty` — mbr copy to short output remains empty.
34. `r090_p14_gpt_rejects_bad_primary_signature` — gpt rejects bad primary signature.
35. `r090_p15_gpt_rejects_bad_primary_header_checksum` — gpt rejects bad primary header checksum.
36. `r090_p16_gpt_rejects_bad_backup_header_checksum` — gpt rejects bad backup header checksum.
37. `r090_p17_gpt_rejects_mismatched_backup_entry_data` — gpt rejects mismatched backup entry data.
38. `r090_p18_gpt_rejects_nil_disk_guid_with_valid_checksums` — gpt rejects nil disk guid with valid checksums.
39. `r090_p19_gpt_rejects_nil_unique_guid_with_valid_checksums` — gpt rejects nil unique guid with valid checksums.
40. `r090_p20_gpt_rejects_invalid_partition_last_lba_with_valid_crcs` — gpt rejects invalid partition last lba with valid crcs.

### F — Durable COW filesystem and recovery (20)
41. `r090_f01_format_blank_creates_two_identical_superblocks` — format blank creates two identical superblocks.
42. `r090_f02_mount_rejects_unformatted_blank_medium` — mount rejects unformatted blank medium.
43. `r090_f03_format_rejects_data_in_last_sector` — format rejects data in last sector.
44. `r090_f04_create_and_remount_preserves_content` — create and remount preserves content.
45. `r090_f05_create_empty_file_survives_remount` — create empty file survives remount.
46. `r090_f06_maximum_payload_survives_remount` — maximum payload survives remount.
47. `r090_f07_oversize_payload_is_not_published` — oversize payload is not published.
48. `r090_f08_duplicate_create_preserves_original_file` — duplicate create preserves original file.
49. `r090_f09_overwrite_existing_file_survives_remount` — overwrite existing file survives remount.
50. `r090_f10_overwrite_missing_file_fails_without_creating` — overwrite missing file fails without creating.
51. `r090_f11_read_missing_file_is_not_found` — read missing file is not found.
52. `r090_f12_short_read_buffer_is_not_partially_published` — short read buffer is not partially published.
53. `r090_f13_deletion_survives_fresh_mount` — deletion survives fresh mount.
54. `r090_f14_delete_missing_does_not_create_metadata` — delete missing does not create metadata.
55. `r090_f15_rename_file_survives_fresh_mount` — rename file survives fresh mount.
56. `r090_f16_create_ignores_no_invalid_parent_escape` — create ignores no invalid parent escape.
57. `r090_f17_create_two_files_preserves_both_after_overwrite` — create two files preserves both after overwrite.
58. `r090_f18_two_corrupt_superblocks_refuse_mount` — two corrupt superblocks refuse mount.
59. `r090_f19_corrupt_new_payload_cannot_publish_file_on_mount` — corrupt new payload cannot publish file on mount.
60. `r090_f20_failing_first_overwrite_preserves_prior_checkpoint` — failing first overwrite preserves prior checkpoint.

### I — Interactive terminal input and process privacy (20)
61. `r090_i01_edit_home_inserts_at_front` — edit home inserts at front.
62. `r090_i02_edit_end_inserts_at_tail` — edit end inserts at tail.
63. `r090_i03_backspace_at_start_is_noop` — backspace at start is noop.
64. `r090_i04_delete_at_end_is_noop` — delete at end is noop.
65. `r090_i05_insert_newline_rejected_without_mutation` — insert newline rejected without mutation.
66. `r090_i06_set_line_invalid_bytes_preserve_old_draft` — set line invalid bytes preserve old draft.
67. `r090_i07_full_length_line_refuses_insert` — full length line refuses insert.
68. `r090_i08_paste_control_byte_rejected_atomically` — paste control byte rejected atomically.
69. `r090_i09_paste_overflow_rejected_atomically` — paste overflow rejected atomically.
70. `r090_i10_range_replace_preserves_prefix_and_suffix` — range replace preserves prefix and suffix.
71. `r090_i11_inverted_replace_range_preserves_content` — inverted replace range preserves content.
72. `r090_i12_literal_replace_all_updates_each_match` — literal replace all updates each match.
73. `r090_i13_literal_replace_overflow_preserves_content` — literal replace overflow preserves content.
74. `r090_i14_selection_copy_preserves_editable_line` — selection copy preserves editable line.
75. `r090_i15_selection_cut_removes_only_requested_range` — selection cut removes only requested range.
76. `r090_i16_clipboard_paste_reinserts_selection` — clipboard paste reinserts selection.
77. `r090_i17_kill_line_preserves_private_clipboard` — kill line preserves private clipboard.
78. `r090_i18_process_forget_scrubs_history_and_clipboard` — process forget scrubs history and clipboard.
79. `r090_i19_history_next_restores_unsent_draft_without_edit` — history next restores unsent draft without edit.
80. `r090_i20_deleting_recalled_command_cancels_history_draft` — deleting recalled command cancels history draft.

### A — ATA PIO status and device identification (20)
81. `r090_a01_absent_controller_status_is_failure` — absent controller status is failure.
82. `r090_a02_all_ones_controller_status_is_failure` — all ones controller status is failure.
83. `r090_a03_busy_without_error_is_pending` — busy without error is pending.
84. `r090_a04_busy_with_error_waits_for_completion` — busy with error waits for completion.
85. `r090_a05_data_ready_requires_drq` — data ready requires drq.
86. `r090_a06_data_ready_rejects_missing_drq` — data ready rejects missing drq.
87. `r090_a07_completion_ready_requires_cleared_drq` — completion ready requires cleared drq.
88. `r090_a08_completion_waits_for_drq_to_clear` — completion waits for drq to clear.
89. `r090_a09_completed_command_error_is_rejected` — completed command error is rejected.
90. `r090_a10_completed_command_drive_fault_is_rejected` — completed command drive fault is rejected.
91. `r090_a11_identify_rejects_missing_lba_capability` — identify rejects missing lba capability.
92. `r090_a12_identify_rejects_zero_sector_capacity` — identify rejects zero sector capacity.
93. `r090_a13_identify_limits_capacity_to_lba28` — identify limits capacity to lba28.
94. `r090_a14_identify_rejects_non_ata_device` — identify rejects non ata device.
95. `r090_a15_identify_rejects_incomplete_response` — identify rejects incomplete response.
96. `r090_a16_identify_rejects_explicit_no_flush_support` — identify rejects explicit no flush support.
97. `r090_a17_identify_rejects_large_logical_sector` — identify rejects large logical sector.
98. `r090_a18_identify_rejects_contradictory_sector_declaration` — identify rejects contradictory sector declaration.
99. `r090_a19_identify_accepts_unreported_optional_sector_size` — identify accepts unreported optional sector size.
100. `r090_a20_failed_staged_pio_read_never_exposes_partial_words` — failed staged pio read never exposes partial words.

## Remaining full M1 acceptance
Normal boot PID1, isolated ring3 Files/Terminal/compositor, physical PS/2-to-authenticated-syscall behavior, safe partition mount, and QEMU disk reboot readback require separate acceptance. **Do not close an M1 gate from host regression tests alone.**

## Exact-SHA CI acceptance record
To fill from GitHub only after workflow completion:
- Full amended commit SHA:
- Required host/combined-regression check:
- Security/no_std check:
- QEMU runtime check:
- Boot ELF/ISO serial check:
- Persistent desktop check:
- Review/coordinate/sha checks:
- Final merge state:
