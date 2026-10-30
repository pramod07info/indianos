# Preserve rejected checkpoint ownership during recovery

A checksum-rejected payload does not make its metadata references reusable.
Previously, a recovery mount discarded that head from COW bank allocation.
Writing identical payload bytes for a different filename could repair the
rejected head's checksum before the new metadata was published. A power cut
then exposed the rejected namespace again, despite unchanged metadata sectors.

`DurableFs::available_bank` now also excludes banks referenced by both
structurally valid observed metadata heads, including a head whose payload
failed verification. The existing stale-head check still runs before any data
write. Publication or explicit redundancy repair replaces the cached head,
which naturally releases obsolete references. Structurally invalid metadata
cannot revive merely through payload writes and does not reserve a bank.

There are two metadata heads and three banks per file slot. Each head references
at most one bank per slot, so at least one bank remains available; recovery does
not consume a file slot or reduce the existing full-volume overwrite capacity.
The disk format, public interfaces, error variants and explicit/no-autoformat
recovery policy are unchanged. No automatic metadata repair is introduced.

## Regression evidence

The new namespace regression failed on the unmodified production algorithm:
after a cut before the second data-sector write, remount chose generation 2
with `/rejected` present instead of generation 1 with neither new file present.
The metadata sectors had not changed. The diagnostic public-API reproducer and
baseline failure log are retained by the coordinator outside the repository.

Tests exercise 400 prepublication cuts across all eight file slots, five payload
sizes (one byte through the full 2 KiB bank), and first/last payload corruption.
They verify the old generation, unchanged metadata, unrelated files, remount
retry and the intended final namespace. Fifteen additional cuts verify the
third bank remains usable when surviving and rejected heads reference distinct
banks. Full-slot repeated overwrites prove obsolete reservations are released;
malformed-head coverage verifies that a bad metadata CRC cannot leak capacity.

Commands:

- `cargo test --locked --offline --manifest-path rust/kernel_core/Cargo.toml --lib durablefs::tests`
- `cargo check --locked --offline --manifest-path rust/kernel_core/Cargo.toml --no-default-features --target x86_64-unknown-none --features test-r031-vfs`

This is deterministic block-device fault injection. Combined real QEMU disk and
reboot acceptance belongs to the coordinator; this task alone does not claim
physical power-cut or hardware qualification. The volume still requires its
existing serialized single-owner, durable-sector-write contract.

## Geometry stability follow-up

Independent review found that decoding cached heads with a later live
`sector_count()` could drop those reservations without changing either metadata
sector. The mount now retains its original validated sector count, uses that
count for metadata encoding/decoding, and rejects changed capacity before any
mutation. Formatting, mount and read-only health validation also require stable
geometry throughout their scans; data/metadata writes and explicit repair
recheck geometry at their transfer boundaries. This does not replace serialized
ownership or guarantee atomicity against device changes inside a transfer.

A regression first failed the quarantine assertion under the preceding fix.
The corrected tests change capacity to zero, below minimum, larger than the
mounted image and beyond the supported 32-bit format limit, then arm a cut
before the second data write. Create, overwrite, delete, rename and repair
refuse the stale geometry without any disk mutation or consumed write budget;
restoring geometry and remounting cannot resurrect the rejected filename.
Additional tests change geometry during format, mount and health reads and
require rejection before attempted formatting writes.

Follow-up validation passed: 35 focused DurableFs tests; 2,298 combined host
regressions with PMM/VMM, graphical durable Files, supervisor, networking, IRQ
keyboard and signed-package security enabled; and the freestanding
`x86_64-unknown-none` check with `test-r031-vfs`. Existing compiler warnings were
retained. These results do not substitute for the coordinator's combined QEMU
acceptance.
