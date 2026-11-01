# IndianOS M1/R076 developer workstation checkpoint

Base commit: d7556ea8b0e67a4c0a216506c4eca50b4dcca0d5

This draft PR contributes real changes toward the original **30-task**
roadmap; it does not claim completion of 30 features.

- Task 15: byte-offset-to-line/Unicode-scalar-column mapping, inverse of
  existing text_offset; checks UTF-8 boundaries and stale identities.
- Task 16 (partial): owner-checked bounded Unicode selection copy, cut and
  paste with a caller-owned clipboard; actual graphical clipboard and
  shortcuts are not yet wired.
- Task 17: backwards literal source search, bounded/paginated match offsets,
  extending existing forward search.
- Desktop integration: read/write capability and expiry checks on these
  source-editing service routes.

Four new Rust tests cover Unicode, invalid boundaries, recycled identities,
expired grants, copy/cut/paste, search paging, and real persistent VFS remount.
The tests have **not been run locally**. Actual pass/fail is determined by
GitHub Actions after opening the PR. QEMU boot is not claimed until validated.

Tasks 01–14 and 18–30 are pending. Task 16 remains partial pending
graphical clipboard integration. No unrelated compatibility work is added.
