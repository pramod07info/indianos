# Reject stale durable filesystem writers

Each DurableFs mount retains the exact two metadata sectors observed at mount.
Mutations compare both sectors with that snapshot before writing a COW bank
and again before publishing metadata. A changed head returns InvalidVolume
without overwriting newer data. Deletes, file/directory renames and explicit
redundancy repair use the same publication fence. Successful commits and repairs
advance the snapshot. The on-disk format is unchanged.

After a failed/torn metadata publication, remount before retrying writes.
Remount explicitly revalidates recovery state. A mount that initially observed
a degraded head may still use the existing recovery behavior; no automatic
repair is added. Read semantics remain unchanged.

The volume owner MUST serialize complete operations across mounts. Snapshot
comparison detects stale sequential writers, not races between a check and
write; it is not a filesystem lock or protection against concurrent DMA.

Host regressions use two mounts of the same memory-backed device, advance the
writer until the stale free bank is live, and verify denied operations issue
zero writes and preserve all sectors. Further tests cover inactive-head changes,
torn publication, remount and successful recovery.
Run: `cargo test --manifest-path rust/kernel_core/Cargo.toml durablefs::tests`.
Device acceptance: `make test-r031-vfs-qemu`.

# Observed validation, 2026-10-29

Default host suite: 1,796 passed, including both new stale-mount regressions.
Targeted `no_std` x86_64-unknown-none check with test-r031-vfs passed.
`make test-r031-vfs-qemu` passed all five stages: 2 KiB range I/O,
hierarchical tree, reboot persistence, corrupted metadata fallback,
sector guard and ring-3 worker.

