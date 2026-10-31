# M1/R070 — five real workstation functional workstreams

Base: verified GitHub main commit at batch start. This release deliberately extends existing kernel-connected, no_std Rust workstation subsystems. No placeholder agents.

1. Disk editor: 4-space indent or strict unindent of 1-based source row ranges. UTF-8-preserving staged edit, one undo/redo operation, durable save/remount, and fail-closed invalid ranges/owner.
2. Persistent project search: one-based matching source line numbers for a Unicode literal across the real COW project file. Read-only, version-checked and short-output-atomic.
3. Desktop integration: authenticated FILE_WRITE formatting and FILE_READ search through the existing PID generation, session and grant gates.
4. Live shell editing: lowercase the command verb without altering case-sensitive file paths, clipboard, caret, submitted history or arguments.
5. Persistent terminal: taillines /path N returns final N complete UTF-8 rows of an authenticated mounted source, with short output and bad UTF-8 remaining atomic.

Release validation: full host Cargo tests; freestanding Rust/no_std build; security checks; actual QEMU desktop boot and durable recovery. The GitHub connector coordinates these five code streams directly; no claim of five independent Codex agents. One source commit and one PR only. Keep draft and do not merge until required CI confirms all gates.
