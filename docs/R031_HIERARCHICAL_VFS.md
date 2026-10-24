# IndianOS R031.3 — Bounded hierarchical VFS on R027 storage

Opt-in x86-64 QEMU milestone using the R027 durable two-metadata-copy,
three-bank-per-file, CRC-validated filesystem without a format change.

Supported: nested directory creation, empty-directory removal, parent
resolution, listing, bounded create/read/overwrite/unlink, and **atomic
cross-directory file rename** as one metadata-generation publication.
All writes require existing parent directories. Absolute path canonicalization
rejects traversal and ambiguous aliases. Kernel syscall 32 is PID1-only and
copies its 64-byte descriptor and user data through validated ring3 memory.
The legacy flat-file API cannot modify reserved `/~d` or `/~f` backing names.

The QEMU test performs four consecutive real boots on the same disposable
128-sector disk, deliberately corrupting the latest metadata checksum
between boot 3 and boot 4. The prior committed generation is recovered,
and existing user files and directories remain valid.

**Bounded scope:** Eight total underlying file entries *including directories*,
maximum 2048 bytes each, 256 bytes transferred per syscall, maximum 21
ASCII bytes of encoded path components. No links, directory rename, mount
namespace, streaming file descriptors, user permissions, filesystem journal,
parallel writers, or actual physical-disk installation. This is a milestone
toward a fuller inode-based general filesystem, not a production VFS.

Run: `make test-r031-vfs-qemu`
