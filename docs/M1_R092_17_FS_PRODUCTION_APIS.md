# IndianOS R092 — 17 new production filesystem APIs

**Scope:** Extend the existing allocation-free Rust kernel COW filesystem with real transactional file operations. Preserve the previous R089–R091 hardening and R090 100-test suite. These functions are *kernel-level APIs*, and a separate runtime change is required before independent ring3 processes can call them.

## New executable production operations (17)

| ID | Rust API | Atomicity / validation |
|---|---|---|
| R092-01 | `verified_len_path` | Full CRC-verified payload read before reporting file length |
| R092-02 | `verified_crc_path` | Recompute CRC32 from checked full file data |
| R092-03 | `equals_path` | Verified content comparison with no disk writes |
| R092-04 | `append_path` | Bound checked atomic COW append |
| R092-05 | `resize_path` | Atomic truncation / zero-filled extension, no sparse holes |
| R092-06 | `fill_range_path` | In-place logical fill via full COW file replacement |
| R092-07 | `insert_range_path` | Atomic insertion and tail shifting |
| R092-08 | `remove_range_path` | Atomic byte-range deletion and tail compaction |
| R092-09 | `splice_path` | Transactional variable-length replacement |
| R092-10 | `copy_path` | Independent new file with verified source bytes |
| R092-11 | `swap_paths` | Exchange names using one metadata commit, no file rewrite |
| R092-12 | `rename_overwrite_path` | Atomically replace existing target name in one metadata commit |
| R092-13 | `compare_exchange_path` | Verified conditional COW update; mismatch is read-only |
| R092-14 | `delete_if_equal_path` | Conditional delete, checksum-verifying entire old file |
| R092-15 | `rename_if_equal_path` | Conditional rename with checked full old contents |
| R092-16 | `copy_range_into_path` | Verified source/destination, overlapping self-copy supported |
| R092-17 | `replace_all_path` | Allocation-free literal byte replacement, whole-file bound preflight |

A private helper `r092_snapshot` stages one verified file into 2 KiB fixed scratch. It is **not** counted as a separate feature.

## Verification
- **26 new `#[test]` functions** target all operations, limits, copy-on-write semantics, remounts, and two fault-injection cases.
- All code is in `rust/kernel_core/src/durablefs.rs`, not checklist-only or throwaway agent source.
- Pending **exact-commit GitHub CI**. Code committed != accepted / verified.
- Existing capacity is 8 files × 2 KiB each; no unsupported large-file claim.
- No physical media formatting. Tests use in-memory simulated block devices.

## Progress against the user's next 90-task request
- Newly implemented full production API operations: **17**.
- Additional full production tasks not completed: **73**.
- Previous 10 R091 safeguards and 100 R090 regressions remain part of the consolidated PR.
- No new M1 gate is declared passed by these APIs alone; service-IPC wiring and physical QEMU boot / post-reboot validation are separate acceptance tasks.
