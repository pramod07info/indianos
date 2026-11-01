# IndianOS M1/R074 — Five substantial workstation workstreams

Based on verified merged R073 PR #704 and archived genuine QEMU screenshots at main SHA d4372f7a325d0b9f1f521d7f7db04a5a5f7cf87a.

## Five real subsystem deliverables

1. **Undoable source-line reordering.** DiskEditor.move_source_line_one() swaps adjacent UTF-8 row bodies while preserving positional newline delimiters (including unterminated final row). One reversible, in-memory edit, persisted only through the existing owner-bound authenticated save. Rejects binary sources, stale PIDs and invalid row indexes.
2. **Live source diagnostics.** ProjectTree.diagnose_source() inspects real durable project files for row count, TODO/FIXME marker rows, trailing whitespace rows, tab-indented rows and missing final newline. Read-only, UTF-8 validated, generation- and owner-bound; not a compiler or language parser.
3. **Desktop capability routing.** Desktop.editor_move_source_line_one() requires an active FILE_WRITE grant, and Desktop.project_diagnose_source() requires FILE_READ; both validate process generation, current session and time before reaching persistent state.
4. **Safe command-argument deletion.** InputLine.delete_argument(index) removes exactly one space-delimited token and its appropriate separator in the live unsent command draft, without triggering execution or clearing successful history/clipboard; invalid index preserves input and caret.
5. **Contextual source inspection.** The authenticated persistent shell gains `sourcecontext /src foo 1`, returning one-based numbered matching rows and up to three lines of surrounding context. Overlapping windows deduplicate, all output is staged before caller publication, corrupt UTF-8 and expired grants fail closed.

## Testing and release safety

Implementation changes five existing Rust production modules and adds this document, with dedicated host tests for UTF-8, newline-edge cases, row deletion, invalid buffer, PID recycling, grant expiry, durable COW mount/remount, and no unexpected filesystem writes. The desktop APIs extend the developer-workstation service; this does not claim that a full graphical IDE wiring exists.

Single integration commit and single PR. Leave draft until required GitHub Actions checks (Rust regressions, no_std, security, real QEMU boot, durable filesystem desktop) pass on the exact current head. Fix CI errors in the integration branch and re-squash to one commit. Merge only after policy and independent review. Never touch physical disks.

Five scoped engineering workstreams were implemented directly through the GitHub connector; no separate pool of five autonomous Codex coding processes was launched.
