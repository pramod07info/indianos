# M1/R069 — five workstation functional workstreams

Based on merged main commit \`349e950d8cc802cf42bb936bd77175cbdd997399\` (PR #693).

## Production functionality

1. **Project workspace** — \`ProjectTree::rename_source_identifier\` atomically rewrites exact ASCII programming identifiers across a real UTF-8 file, rejects invalid identifiers, preserves longer names and avoids unnecessary durable writes. Includes COW remount and stale identity tests. The operation is lexical, not a syntax-aware language server, so matches in comments/strings are possible.
2. **Text editor** — \`DiskEditor::trim_source_trailing_whitespace\` cleans source rows (including Unicode) in one bounded, undoable/redoable unsaved edit; persistence uses the existing owner-bound \`save\` and COW mount.
3. **Authenticated developer desktop** — source cleanup and project rename are routed through \`admit_editor(..., FILE_WRITE)\` before reaching storage or editor buffers. Recycled owners and expired grants cannot change source.
4. **Live interactive terminal input** — \`InputLine::erase_to_home\` / \`LineEditor::erase_to_home\` implement Ctrl-U-style prefix deletion at the actual caret, preserving unsent suffix, successful history and clipboard, and scrubbing removed data.
5. **Persistent terminal diagnostics** — \`headlines /path N\` returns the first N complete UTF-8 rows; \`rfind /path text\` reports last-match byte offset. Both require FILE_READ, read actual mounted storage and refuse partial output on invalid input.

This integration touches **five existing production Rust modules and this document**, with 14 new Rust regression tests. There are no placeholder \`agent_XX.rs\` files or disabled tests. Workstreams were implemented by one GitHub coordinator; **five separate autonomous Codex workers were not launched** by the available GitHub connector.

## Release gating

This submission is a **DRAFT**, not a claim of working physical hardware or accepted M1 UI:
- Exact PR-head host Rust build and all tests, including new regressions.
- Freestanding no_std kernel build; M1/S0 security checks.
- Genuine QEMU boot, desktop Files/Terminal and durable source recovery after reboot.
- Independent review, repository required checks and a clean integration diff.
- Fix failures in code on the same branch, re-squash to a single commit before merge. Do not write or format any physical disk.

Only merge once all required checks are verified green. Do not report unverified gate completion or infer five independent agents.
