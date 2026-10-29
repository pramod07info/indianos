# M1 ATA IDENTIFY admission

The experimental primary-master PIO driver transfers exactly 256 words per
sector. Before this change `capacity()` checked only word 49 bit 9 and words
60/61: a response with word 106 = `0x5000`, word 117 = `2048`, word 118 = `0`
was accepted as a 512-byte block device despite declaring 4096-byte logical
sectors. Short reads leave data pending; short writes fail to supply a sector.
The production parser now rejects that response before constructing `AtaPio`.

Admission requires an ATA (word 0 bit 15 clear), complete (bit 2 clear) identity,
LBA support, and nonzero capacity. Capacity remains capped at 2^28 sectors.
Word 106 is meaningful only when bits 15:14 are `01`. With a valid word and
bit 12 set, words 117/118 supply the logical sector size in 16-bit words:
values <=256 contradict the declaration; larger values are unsupported.
Without that declaration, ATA's legacy 256-word logical size applies. Invalid
or absent optional-word validity signatures do not activate stale size fields.
Physical-sector multiples (512e, word 106 bit 13) do not change transfer size.
4Kn and other larger logical sectors are unsupported.

A valid word 83 explicitly lacking bit 12 (FLUSH CACHE support) is rejected.
Older devices without a valid optional command-set declaration are not rejected
solely for that absence. Every write still requires FLUSH CACHE completion;
command errors return `BlockError::Io`. This is not proof of physical durability:
IDENTIFY may lie, and host caching, QEMU backing files and physical media remain
outside this driver. No physical-disk or general hardware support is claimed.
Probe now also requires IDENTIFY's data phase to complete before admission.

The explicit caller-selected, single-core QEMU IDE disposable-test-device scope
and exclusive port ownership requirement remain unchanged. Regular boots must
not probe physical disks. Parser rejection uses the existing `BlockError::Io`
probe error without exposing a usable device; no reset, retry, or fallback write
is attempted.

Focused tests exercise the actual production parser with legacy and declared
512-byte geometry, 512e, 4Kn, malformed zero/short/huge sizes, all optional-word
validity patterns, incompatible identity/flush capability, and LBA28 boundaries.
These synthetic tests perform no port I/O; QEMU runtime coverage is separate.

Validation performed with locked, offline Cargo and the session Rust toolchain:

- `cargo test --locked --offline --manifest-path rust/kernel_core/Cargo.toml ata_pio::tests`: 9 passed.
- `cargo check --locked --offline --manifest-path rust/kernel_core/Cargo.toml --no-default-features --target x86_64-unknown-none --features test-r031-vfs`: passed (existing warnings).
- A standalone test of the base commit's extracted, unchanged `capacity()` with
  the 4Kn response above failed as expected: actual `Some(1024)`, expected `None`.
