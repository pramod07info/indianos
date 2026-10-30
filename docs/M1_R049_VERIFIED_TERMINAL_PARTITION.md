# R049 — M1 verified developer Terminal partition sessions

## Scope and actual work
This change strengthens the **M1 G1/G2/G6 integration boundary** on existing kernel code. New APIs perform live, identity-checked disk mounting and authenticated persistent commands rather than adding another set of superficial agent slots.

- `command_on_checked_partition` verifies the exact previously selected MBR/GPT ordinal and geometry and both healthy COW metadata banks.
- `command_on_gpt_selection` verifies disk GUID, partition GUID, geometry, both GPT tables, and healthy metadata on every invocation. A swapped/repartitioned disk with identical LBAs cannot reuse the original selection.
- All three partition-terminal routes preflight the caller's live process identity, generation, grant expiration and minimum filesystem scope **before any disk read**, then retain verb-specific authorization inside `command`.
- Durable Desktop adds an explicit read-only GPT identity + metadata health preflight; callers can pass the token to its existing strict GUID-pinned mount API.
- Eight regression tests cover cross-remount write/rename/read, changed partition GUID, changed disk GUID, stale MBR geometry, degraded metadata, expired/recycled PID, unformatted media, read-only permissions, output atomicity and neighbor sector safety.

## Truthful implementation status
This is **one coordinator-produced functional slice** with no independently launched coding agents in the current GitHub connector. The 20 priority workstreams from [M1_RELIABILITY_BATCH.md](M1_RELIABILITY_BATCH.md) remain separately tracked; their implementation must not be claimed because of this PR. Existing index-only command entrypoints remain for compatibility; they now require a live filesystem grant and do not silently switch to strict GUID-pinned behavior.

## Merge gates
Keep the PR draft until exact-head offline Rust host/no_std checks, M1 QEMU durable Files/Terminal, kernel boot, security S0, independent review and current-main CI pass. Never use physical disks or suppress failed checks. This is a bounded kernel-service API; **not** yet a genuine ring-3 syscall ingress, an independent userland compositor or full M1 gate acceptance.
