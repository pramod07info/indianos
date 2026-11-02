# IndianOS R095–R099 — Final 49 kernel COW/VFS/handle coding tasks

## Exact scope
- User requested **49 additional production coding tasks**, completing the 90-item code workstream started with R092.
- The following **49 distinct production methods/dispatch capabilities** have been added to existing Rust kernel source. Some APIs expose lower-layer capabilities to other layers, so **49 coding deliverables are not 49 independently finished OS features or 49 closed boot gates**.
- R095–R097 add **31 new DurableFs methods**, R098 adds **10 hierarchical VFS methods**, R099 extends authenticated FileHandles with **8 actions**.
- **54 new focused Rust tests** are included (30+12+12). The source is committed in a single consolidated PR revision; CI results for its exact new SHA, host tests, no_std, QEMU/reboot/ELF boot, remain mandatory.
- All mutating data operations remain allocation-free in kernel code, use 2 KiB maximum file payloads, copy-on-write publication, verified metadata/data reads, bounded slices, and overflow preflight.
- Input and output for FileHandles actions are PID-incarnation and opaque-token bound; PID2 remains read-only; action 24 is guarded by the service-wide write budget and policy.

## 49 individually traced coding deliverables

### R095 — Atomic file transformations (12)
*File:* `rust/kernel_core/src/durablefs.rs`
01. `R095-01` — `prepend_path`
02. `R095-02` — `drop_prefix_path`
03. `R095-03` — `drop_suffix_path`
04. `R095-04` — `replace_prefix_path`
05. `R095-05` — `replace_suffix_path`
06. `R095-06` — `reverse_range_path`
07. `R095-07` — `rotate_left_path`
08. `R095-08` — `rotate_right_path`
09. `R095-09` — `swap_ranges_path`
10. `R095-10` — `move_range_path`
11. `R095-11` — `xor_range_path`
12. `R095-12` — `uppercase_ascii_range_path`

### R096 — Verified file reads/search and inspection (10)
*File:* `rust/kernel_core/src/durablefs.rs`
01. `R096-01` — `find_bytes_path`
02. `R096-02` — `rfind_bytes_path`
03. `R096-03` — `count_bytes_path`
04. `R096-04` — `has_prefix_path`
05. `R096-05` — `has_suffix_path`
06. `R096-06` — `read_byte_path`
07. `R096-07` — `read_head_path`
08. `R096-08` — `read_tail_path`
09. `R096-09` — `range_equals_path`
10. `R096-10` — `verified_empty_path`

### R097 — Metadata-generation guarded COW transactions (9)
*File:* `rust/kernel_core/src/durablefs.rs`
01. `R097-01` — `append_if_generation_path`
02. `R097-02` — `insert_if_generation_path`
03. `R097-03` — `remove_if_generation_path`
04. `R097-04` — `splice_if_generation_path`
05. `R097-05` — `fill_if_generation_path`
06. `R097-06` — `delete_if_generation_path`
07. `R097-07` — `swap_if_generation_path`
08. `R097-08` — `rename_if_generation_path`
09. `R097-09` — `copy_if_generation_path`

### R098 — Hierarchical Files/VFS integration (10)
*File:* `rust/kernel_core/src/hier_vfs.rs`
01. `R098-01` — `find_bytes_file`
02. `R098-02` — `rfind_bytes_file`
03. `R098-03` — `count_bytes_file`
04. `R098-04` — `has_prefix_file`
05. `R098-05` — `has_suffix_file`
06. `R098-06` — `read_head_file`
07. `R098-07` — `read_tail_file`
08. `R098-08` — `prepend_file`
09. `R098-09` — `reverse_file_range`
10. `R098-10` — `append_if_generation_file`

### R099 — Authenticated PID-bound file ABI actions 17–24 (8)
*File:* `rust/kernel_core/src/fs_handles.rs`
01. `R099-01` — `action_17_find_first`
02. `R099-02` — `action_18_find_last`
03. `R099-03` — `action_19_count`
04. `R099-04` — `action_20_prefix`
05. `R099-05` — `action_21_suffix`
06. `R099-06` — `action_22_read_head`
07. `R099-07` — `action_23_read_tail`
08. `R099-08` — `action_24_prepend`


## Acceptance criteria and limitations
1. Every changed Rust module must compile with the actual existing crate, no suppressed or ignored tests.
2. Host regression suite and its new file-operation cases must pass on the **exact final commit SHA**.
3. Security/no_std, persistence, runtime-QEMU and boot ELF/ISO/serial gates must pass; no merge before required checks are green.
4. No separate autonomous agents or physical disk formatting are claimed.
5. The 90 coding work items across R092 (17), R093 (14), R094 (10), R095–R099 (49) are implementation/API tasks, **not acceptance of the eight high-level pending M1 gates**, which require normal PID1, independent ring3/files/compositor, hardware input, durable reboot verification and hardware/QEMU qualification.
6. R099 action 17/18 returns `u64::MAX` for no match; action 24 resets the handle cursor after the new prepended prefix; action 22/23 are bounded read-only peeks and do not advance the cursor.
