# M1 R048: bytewise GPT mirrors and GUID-pinned workstation mounts

Improves **M1 G2** under #616. It does not complete that gate or any of eight outstanding high-level M1 gates.

## Production changes
* Read and compare all used sectors of primary and secondary GPT entry arrays **bytewise**, in addition to their two independent CRCs. CRC32 collisions must not mask divergent partition metadata.
* Return validated GPT disk/unique partition GUID plus geometry as an explicit selection token.
* Verify identity again for healthy, explicit GPT mounts even if ordinal and geometry match. Surface the strict mount in the durable Desktop API; MBR has no guaranteed partition GUID and is not supported by this token.
* Five Rust regressions exercise forged equal-CRC mirror disagreement, media identity changes, persistent readback, no-write guarantees and MBR rejection.

## Verification and limits
Require offline Rust unit tests, freestanding no_std compilation, exact-head kernel boot, M1 disposable-disk real QEMU desktop/reboot, S0 security and independent review. Do not merge on pending/skipped/failed tests. Existing geometry-only mount APIs remain available for compatibility and are **not** silently strengthened. This is not a production installer, normal non-test desktop boot or physical-hardware acceptance. No autonomous coding agent is claimed to have been launched.
