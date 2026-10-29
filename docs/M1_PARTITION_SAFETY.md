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
