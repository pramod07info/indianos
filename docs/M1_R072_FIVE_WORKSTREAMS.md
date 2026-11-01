# IndianOS M1/R072 — five substantial Rust workstation workstreams

Starting point: merged R071 PR #700 and the corresponding actual-QEMU screenshot archive, main SHA ae146a0f0aba3c38c65d0f020af15d155ba14c61.

## Production features

1. Editor tab indentation normalization: expand only *leading* source tabs into four spaces while retaining interior tabs and UTF-8. One unsaved editor mutation supports owner-bound undo, redo and later durable save. Invalid binary input and recycled owners fail closed.
2. Optimistic single-line source editing: owner-bound project API compares the exact old source row, replaces only that row, preserves final-line newline layout, and makes one durable COW commit. Rejects external generation changes, mismatched row, invalid Unicode and file-size overflow.
3. Authenticated desktop integration: both editor indentation and project row compare-and-swap are accessed through the real desktop session, PID-generation and time-bounded FILE_WRITE gates.
4. Shell draft argument reordering: swap adjacent whitespace-delimited tokens without executing the draft, changing saved history or clearing clipboard; preserve intervening whitespace and case-sensitive path tokens.
5. Durable source search from terminal: `linehits /src foo` prints 1-based matching row numbers as a space-delimited list. It reads the real mounted COW source, validates UTF-8 and the FILE_READ grant and publishes only after fully checking output bounds.

These are direct changes to five existing Rust production modules (plus documentation), not superficial agent_XX.rs files. Each subsystem has real regression tests including Unicode, persistence across remount, rejected permissions, stale sessions, optimistic concurrency and short output.

## CI and integration policy

Exactly one source commit and one PR; do not merge before verified host Cargo tests, no_std freestanding build, security gate and genuine QEMU boot/persistent desktop checks. On failure inspect exact logs, fix existing branch and re-squash to one commit; never disable tests or claim gates without GitHub verification. No physical-disk writes.

Five scoped functional workstreams were integrated through GitHub. This connector did **not** spawn five independent autonomous Codex coding processes.
