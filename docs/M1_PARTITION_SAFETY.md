# Validate and decode the same GPT table bytes

GPT discovery previously made separate entry-array passes for checksum,
mirror comparison and partition decoding. A changing block backend could
return checked bytes first and different unchecked partition geometry later.

Discovery now reads each primary and backup sector once, compares their
meaningful bytes, accumulates both CRCs and decodes entries from that same
primary buffer. The bounded inventory stays private until both complete CRCs
match the checked headers. Final partial sectors are included correctly.
The public partition API and supported GPT format are unchanged.

All discovery remains read-only. This binds the returned inventory to the
observed bytes; it is not cryptographic disk authentication or a lock against
media replacement after discovery. Existing GUID-pinned selection and bounds
checks remain required, and physical-disk writes remain unsupported.

Regressions cover a backend that changes on later reads, divergent mirrors,
matching altered tables with stale CRCs, final partial sectors, and zero writes.
The focused partition suite passed 32 tests and the freestanding check with
test-r031-vfs passed. Combined reboot/mount tests run on the integration SHA.

```sh
cargo test --locked --offline --manifest-path rust/kernel_core/Cargo.toml \
  --features test-r031-vfs m1_partition::
```

## Protective-MBR geometry and QEMU fault media

The GPT protective MBR must start at LBA1 and cover exactly the remaining disk
capacity, saturated at `u32::MAX` only when the disk is larger than that field.
An oversized protective entry on a small disk is rejected before GPT selection,
just like a truncated entry. Regressions include zero/short/long coverage and a
bounded virtual large disk that proves valid saturation remains supported.

The host-only disposable fixture tool can create valid-checksum layout faults:

```
indianos-desktop-fixture --partition-fault overlap --output NEW.raw
indianos-desktop-fixture --partition-fault duplicate-id --output NEW.raw
indianos-desktop-fixture --partition-fault protective-overflow --output NEW.raw
```

Both GPT headers and entry arrays retain valid matching CRCs, the first selected
entry remains unchanged, and the selected COW filesystem remains healthy. The
overlap case introduces a second overlapping entry; the duplicate-ID case uses
separate in-range extents with the same unique GUID. Only partition metadata
changes. Tests verify each precise rejection and repair only the named fixture
fault as a positive control. Existing files, symlinks and physical devices cannot
be overwritten because output uses the same new-file-only `.raw` writer.

These fixtures support actual QEMU admission/rejection evidence. Their presence
does not itself close G2: combined exact-SHA host, boot/remount, failure and CI
evidence must still pass before the coordinator updates its acceptance ledger.
