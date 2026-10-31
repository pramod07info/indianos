# IndianOS M1/R064 — five functional workstation improvements

Base `17a3d18b5ab76fe3f04f363b7c92bfcdfbee2282` (merged #684).

This change integrates five distinct implementation workstreams into ONE
source commit; it does not imply that five independently running Codex agent
processes existed. This GitHub connector cannot provision such sessions.

## Production deliverables

1. **RAM Files:** `RamFs::append_batch_if_versions` performs bounded, optimistic,
   per-file revision-checked appends against an unpublished shadow filesystem
   and publishes all changes only when the entire batch succeeds. RAM-only,
   not disk durable. 3 regression tests.
2. **Disk-backed source editor:** `DiskEditor::replace_source_line`
   replaces a one-based Unicode row, preserving newline boundaries and
   producing one undoable edit; COW save and simulated remount are tested.
   3 regression tests.
3. **Desktop geometry:** `SurfaceLayout::tile_vertical` partitions a region
   into authenticated nonzero-height panes, distributes surplus pixels,
   and commits geometry/epochs through the existing atomic batch interface.
   2 regression tests.
4. **Interactive Terminal:** `InputLine::complete_common_history_prefix`
   extends ambiguous drafts to the longest shared successful-command
   prefix without executing a command or modifying clipboard/history.
   3 regression tests.
5. **Persistent Terminal diagnostics:** `bytefreq /path HH` counts a
   binary byte specified by two hexadecimal digits in an authenticated
   disk-backed file, surviving remount; short outputs and bad inputs
   fail closed. 3 regression tests. Updates the command registry count.

**14 added Rust regression tests**, five existing Rust production modules,
one new documentation file. No empty agent marker files. No tests skipped.
This is host API progress and must not be misreported as completed
M1 user-visible guest acceptance.

## Required checks before merging

- Exact integration head: Rust host compilation and 14 new regression tests.
- Existing no_std freestanding kernel compilation and combined regression suite.
- Security S0 and identity/path/authorization tests.
- Real QEMU normal boot, Desktop/Terminal/Files acceptance and disk persistence.
- Independent review and branch protection.
- No physical disk formatting. Do not merge on pending or failing checks.

If CI fails, fix the production issue in the existing R064 integration
branch and re-squash to ONE commit before merge.

## R064 CI recovery (first exact-head run)

Initial exact-head host regression: 2,377 passed / 1 failed in
`r064_vertical_tiles_tests::unauthorized_window_duplicate_and_offscreen_area_roll_back`.
The failing input `y=700, height=100` is valid on this screen; the
assertion incorrectly treated it as offscreen. The corrected regression
uses `x=u32::MAX, width=60`, a guaranteed overflow/invalid geometry.
Production tiling logic, authorization checks and tests remain intact.
The branch commit was regenerated with the original main parent to
preserve **exactly one source commit**; new CI must independently verify.
