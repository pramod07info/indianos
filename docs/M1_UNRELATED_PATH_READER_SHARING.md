# Selected-volume path mutations with unrelated readers

The selected COW volume still has a global generation and no persistent inode
IDs. Previously, every open descriptor in another process therefore prevented
every path mutation, including creating an unrelated file. This extension uses
the existing serialized service and operation guard; it does not infer object
identity from matching paths or content checksums.

Before any device probe, a path mutation checks both its source and destination
namespace footprints against foreign descriptors. Component-aware overlap in
either direction is rejected. Rename and removal therefore cannot move, replace
or delete a foreign reader's file or its containing tree. `/shared/note` does
not accidentally protect the separate name `/shared/note2`.

After a healthy receipt-bound mount and before authorization or mutation, the
service verifies each foreign lease against its exact cached handle and
namespace receipt. Its owner must still be live, its rights must be READ, and
its recorded filesystem generation must equal the newly mounted generation.
An uncoordinated generation change cannot be repaired by an unrelated write.
The reader must close its stale descriptor or undergo ordinary retirement.

After successful publication, the same authority proof is checked again. Only
the verified untouched foreign leases advance to the new volume generation;
their token, process incarnation, cursor, range lock and namespace stay intact.
Calling-owner handles retain their existing revocation behavior. A transport,
integrity or post-publication authority failure quarantines the selected mount
and revokes every handle. Explicit remount never revives an old token.

This proof depends on exclusive service serialization and the existing trusted
COW generation contract. It does not detect a malicious external writer forging
the same generation, provide SMP synchronization, or introduce per-file durable
identifiers. Descriptor mutation APIs remain conservative volume-wide leases;
only the selected path API gains unrelated-reader sharing.

Descriptor ABI version 1 and the 64-byte descriptor layout are unchanged.
The operation-37 feature mask adds bit 3 (`FS_ABI_UNRELATED_READERS`, value 8)
so a client can negotiate this behavior before holding a read lease across
unrelated broker mutations. Older kernels need not advertise the capability.

Regression coverage includes exact token/cursor/lock preservation across real
selected-volume create, overwrite, rename, directory move and removal; protected
source and destination denial before device access; stale external generations;
PID/namespace changes and quota denial; and write-failure quarantine followed by
fresh remount without token revival. Cloud Rust results must be recorded for the
integrated revision before acceptance.

The focused cloud command `cargo test --locked --offline --manifest-path
rust/kernel_core/Cargo.toml --lib --features normal-desktop fs_handles` passed
169 tests with zero failures after this change. The initial run caught a test
assertion expecting a volume error for an already revoked token; the existing
API correctly returned its stale-token error. The test now verifies one actual
failed device write, quarantine, revocation and unchanged write accounting
before checking that token denial. Combined compilation and guest acceptance
remain required on the integrated head.
