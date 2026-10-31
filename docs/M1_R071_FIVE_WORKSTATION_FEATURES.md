# M1/R071 — five Rust developer workstation workstreams

Base: merged R070 PR #698, including its archived screenshots, main SHA 30a2e2a39979cccd35e354182c074f983f01459c.

## Functional workstreams

1. Text editor: normalize validated UTF-8 CRLF endings to LF within an open source file as one reversible unsaved edit. Bare carriage returns, invalid UTF-8 and recycled identities fail without modifying saved bytes.
2. Project workspace: preview exact lexical identifier rename occurrences and resulting file size BEFORE source modification; read-only, disk-generation checked, valid-identifier bounded. The scanner is lexical, not AST-aware: comments and string literals are included.
3. Desktop: expose newline normalization only with FILE_WRITE and project refactor preview only with FILE_READ, checking authenticated desktop session, PID generation and current access grant.
4. Live terminal input: navigate directly to whitespace-delimited argument index in unsent command text, preserving path case, history and clipboard, without issuing commands or pretending to support quote parsing.
5. Durable terminal: linespan /source 2 3 returns complete UTF-8 source rows from real COW-backed files. Range, permission and output buffer must validate before publication; no partial output.

Code modifies five existing production Rust modules, with real unit and mount/reboot-style integration tests; no placeholder agent_N files. These are five planned engineering workstreams integrated through GitHub, not five separately launched autonomous Codex agents.

## Release gate

PR stays draft until exact PR-head host Cargo tests, freestanding Rust build, S0 security tests, real QEMU kernel/desktop boot and persistent storage recovery pass. Inspect every failure; fix code on the same branch, and preserve exactly one source commit. Merge only after independent review and repository-required checks. No physical disk I/O.
