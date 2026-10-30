# IndianOS M1/R059 — Five real workstation workstreams, one commit

Starting from main `5c22eb4ccb45d968d5072bb94607b22b35cbe19e`, after merged PR #673 (R058) plus its QEMU screenshot archive.

| Workstream | Actual production deliverable | Added regression tests |
| --- | --- | --- |
| Multi-file RAM deletion | `RamFs::delete_batch_if_revision` compares global state and per-file versions and commits a staged deletion set only if *all* files are valid and available. Duplicate paths, missing files, stale versions or wrong global generation leave everything untouched. The invoking kernel service must authorize each path. RAM only, not durable. | Successful two-file batch preserves unrelated data; stale/duplicate batch rolls back; missing/unsafe batch rolls back |
| Durable IDE indentation | `ProjectTree::adjust_source_indent` modifies a selected one-based source line range, adding or removing 1–8 ASCII leading spaces per row. Validates authenticated process, snapshot generation, UTF-8, bounds, required existing indentation, and file-size cap before a single disk-backed COW file overwrite. Supports Unicode and a final line without newline. | Unicode indent and unindent across actual filesystem remount; stale, invalid, partial-unindent and capacity failure retain disk generation |
| Terminal command history search | `InputLine::history_search_previous` searches previously submitted, bounded commands by a literal printable prefix, from newest backward (or older than current history cursor). A missed search never changes active editor or saved draft. Original unsubmitted line is recovered with existing `history_next`. No history leaks across `forget`. | Prefix reverse search including draft recovery; malformed/no-match search and process teardown |
| Desktop surface geometry | `SurfaceLayout::set_geometry_batch` atomically moves and resizes a collection of owner-authenticated windows to validated screen-contained rectangles. Shadow layout ensures all-or-nothing updates to both coordinates and epochs. Duplicate window targets denied, unchanged rectangles do not consume epochs. | Valid resize/reposition and epoch policy; invalid second owner/geometry/duplicates leave all surfaces and epochs unchanged |
| Persistent Terminal binary diff | Authenticated `diffat /left /right` reports first differing byte offset of two real checksummed HierVfs files (or `equal`). For proper prefixes the difference is the shorter length. Full-buffer preflight and bounded decimal output prevent partial client output; the operation is read-only and survives reboot remount. Registry and tools help updated. | Mismatch, prefix and equality across filesystem remount; short response, unauthorized owner, invalid path and nonexistent file are fail-closed |

## Integration policy

- Five substantive workstreams in **five existing production Rust files**, **11 new regression tests**, and one documentation file.
- Single consolidated Git commit, draft PR, gated merge. No dummy agent files, placeholders, skipped tests, or destructive physical hardware operations.
- **Independent coding agents actually launched: 0.** Five workstreams were delivered by a single connected GitHub coding coordinator. The connector cannot spawn autonomous Codex worker sessions.
- No local Rust compiler/QEMU available in this environment. Newly authored tests and implementation **remain unverified** until exact-head GitHub CI completes.
- Required CI: Rust host/no_std build and unit regressions, integrated kernel ELF/ISO/serial boot, security S0, M1 persistent graphical desktop Files/Terminal and QEMU tests, roadmap SHA, independent review, branch protection.
- Service policy and bounded local functions do not establish finished desktop UX, GPU acceleration, all-hardware support, or M1/M2 completion percentages.
