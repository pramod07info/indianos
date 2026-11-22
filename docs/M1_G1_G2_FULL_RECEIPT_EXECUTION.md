# M1 G1/G2 full-receipt command execution hardening

**Scope:** Exact selected-media integrity at the G1 Terminal -> G2 partition boundary, based on `main` commit `ffa99c46c8d3d3164b1edf936cbe55226f250e7f`. This does not close either gate by itself.

## Production change

Previously, the ordinal, checked-partition and GPT-selected Terminal command entrypoints re-read the selected partition after command execution but only compared its index/bounds or GUID tuple. A *different, valid* GPT header/entry epoch with the same GUIDs and ranges, or an MBR boot-record metadata change with identical extents, could therefore produce a false success acknowledgment after a COW write.

All three entrypoints now compare the complete capacity-pinned `PartitionReceipt` before and after executing the command. A GPT Terminal session additionally retains the full receipt from open/recovery, preventing reuse after a same-GUID metadata epoch change *between* commands. Checked and GPT-selected APIs also mount against that exact receipt through the existing healthy-only mount contract. The older ordinal API retains its prior permissive degraded-volume mount contract while adding the full before/after receipt check. On changed metadata the output buffer is cleared and an explicit partition invalidation is returned. A write may already have occurred: this prevents a false ACK, **not** a transaction rollback or physical-device attestation.

## Executable regression cases

- `m1_g1_g2_mbr_metadata_epoch_change_rejects_ack_even_when_geometry_matches`: both ordinal and checked-selected MBR commands are denied after a post-write MBR boot-record change, even though partition geometry is unchanged; no false reply.
- `m1_g1_g2_gpt_metadata_epoch_change_retires_terminal_session`: a valid GPT table/header CRC epoch switch changes an entry attribute during a COW write while preserving GUIDs and bounds. The command refuses success, clears output and retires the session.
- `m1_g1_g2_session_rejects_valid_gpt_epoch_switch_between_commands`: after a successful session open, valid CRC-checked GPT attributes change while GUID and bounds stay identical; the next command fails before any data write, with the session revoked.

All three cases use real block-backed `HierVfs` test volumes and execute the production Terminal paths, rather than a mock returned success.

## G1/G2 exact-head acceptance required before marking complete

1. Host `cargo test --locked --offline --manifest-path rust/kernel_core/Cargo.toml --lib --features normal-desktop`, freestanding `x86_64-unknown-none` and all G1/G2 partition, terminal, permission and crash tests pass at the exact PR SHA.
2. The mandatory M1 combined reliability and normal desktop workflows pass on the exact same SHA. The normal desktop suite must exercise actual PS/2 physical-origin Terminal write/mkdir/rename/read, Files pixel updates, selected GPT partition and neighboring-sector protection, two cold QEMU boots with committed-file readback, altered GPT/MBR media rejection, failure-injected ATA writes and read-only recovery. Retain serial, PPM and device artifacts.
3. S0 security and full kernel boot CI pass at the exact PR SHA; no test or branch-rule bypass. Independent review must inspect this diff, GPT mirror metadata, write-after-device-swap semantics, filesystem privileges and error acknowledgment behavior.
4. After an approved merge, rerun the mandatory checks on the merged `main` SHA and reconcile `docs/DELIVERY_MILESTONES.md` with evidence links. Do not mark G1 or G2 complete on the basis of source changes or local-only tests.

**Out of scope:** physical disk power loss, general file locks/ACLs, driver/DMA qualification (G6–G8). No physical disk writes or automatic filesystem formatting are introduced.

## Additional G2 negative media acceptance
The host-only fixture adds `nil-disk-guid`, `backup-disk-guid` and
`backup-table-mismatch`: all retain healthy COW data, and each GPT header
and its own entry array has valid CRCs. The mirrors nevertheless fail
cross-checks or use an invalid nil identity. The Rust regression verifies
read-only rejection and positive recovery after fixing only the fault.

The mandatory real normal-desktop QEMU harness runs all three new variants
with a write-failing backend, rejects service startup, requires read-only
recovery, and compares the entire disk after boot. This extends the
configured QEMU matrix from 22 to 25 actual boots. **Configured does not
mean passed** until the exact-head GitHub job finishes successfully.


## Follow-up: pinned MBR command receipts after PR #790

The existing ordinal and checked-MBR entrypoints validate a complete table
within each command, but an MBR boot-record metadata change *between* two
commands can preserve the ordinal and geometry. To support trusted callers
that need stable selection over a multi-command operation, the new
`command_on_partition_receipt` API accepts a retained
`inspect_partition_receipt` result from the original explicit selection.

Each call authenticates the scheduler identity before disk reads, requires a
healthy mount of precisely that receipt, then rechecks the receipt after
execution and scrubs acknowledgments on media changes. Three targeted Rust
regressions cover a same-geometry MBR epoch switch between commands, stale
identity with zero disk reads, and a post-write metadata change with no false
success acknowledgment. The tests operate on disposable block-backed HierVfs.

This is an opt-in trusted kernel service; callers must retain the original
receipt rather than discover a new one for each command. MBR has no globally
unique disk identity, and a collision with byte-for-byte identical MBR data on
a replaced device cannot be excluded by software receipts alone. No format
or disk repair is introduced. Exact-commit CI, independent review, and
merged-main verification remain mandatory; no gate completion is asserted.


## G1/G2 follow-up: automatic multi-command MBR session enforcement

The opt-in MBR command receipt from the preceding section is now also
retained internally by `MbrTerminalSession`. It requires a healthy existing
MBR partition and valid scheduler-derived process rights before selection;
there is no re-selection on each command. Every command validates the original
MBR sector, all partition geometry, the device capacity and mounted COW data.
Any changed receipt or uncertain disk I/O permanently retires the session.
The same open/execute/status/shorten/close lifecycle and owner-generation
restrictions as the existing GPT terminal session apply.

Three additional disposable block-backed regressions check: real file writes
and reads across independently remounted commands followed by revocation on a
same-bounds MBR epoch change; stale identity, expiration and owner-only close
with zero unauthorized disk reads; and failed COW writes that revoke the
session until an explicit fresh open. This remains a trusted kernel-service
API rather than proof of a new native ring-3 MBR desktop boot; no autoformat or
physical disk modification. Required exact-head CI and independent review
remain mandatory.
