# Partition discovery capacity snapshot

MBR/GPT discovery now takes one capacity snapshot before reading metadata and
checks the actual device capacity immediately before and after every sector
transfer and once more before publishing a catalog. Its private adapter exposes
only that snapshot to the parsers and always rejects writes. All range checks
use the same capacity. A geometry change returns an I/O error, exposes no
partial catalog, and stops further sector reads. A successful transfer whose
post-read capacity check fails is scrubbed before returning the error.

Previously, media shrinking to zero after sector 0 could panic in protective-MBR
coverage subtraction. A growing MBR device could instead return a catalog from
a different geometry than the one checked at discovery entry. Tests reproduce
both failures on the old production implementation.

Regression coverage includes shrink/growth/removal at every GPT read (primary
and backup headers and both arrays), MBR geometry changes, changes before a
transfer, zero-capacity media, out-of-range reads, write denial, buffer scrubbing,
stable GPT discovery and propagated device errors. No sector writes occur.

The existing caller serialization contract remains required. This detects
observed capacity changes; it cannot detect a device that changes and restores
capacity entirely between observations, or prove identity from capacity alone.
GUID-pinned mounts provide the separate existing GPT identity checks. Discovery
is not a media lock, filesystem implementation, GPT repair, physical-hardware
qualification, or completed M1 partition gate. No physical disks are used.

Commands:

```sh
cargo test --locked --offline --manifest-path rust/kernel_core/Cargo.toml --features test-r031-vfs m1_partition::
cargo check --locked --offline --manifest-path rust/kernel_core/Cargo.toml --no-default-features --target x86_64-unknown-none --features test-r031-vfs
```
