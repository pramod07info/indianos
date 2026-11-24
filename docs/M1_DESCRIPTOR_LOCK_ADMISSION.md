# G6/G7 descriptor lock admission

Classification: fix-existing. The baseline at
`7b2184749db056bf4d19e3f1c3d162179fc71440` already provides authenticated
descriptor locks, persistent discretionary permissions and an ABI version
query. This change extends that lock table; it does not add another locking
API or change the filesystem ABI.

Sequential reads and writes already checked their cursor ranges. Positioned
reads/writes, prefix/head reads and fill operations instead encountered a
whole-file check in mounted dispatch, so a lock on unrelated bytes denied
valid requests. Native selected-volume preflight also did not reject overlap
until after device probing and mounting.

One shared admission function now checks the logical requested span from
kernel-owned handle metadata before media or ACL lookups and repeats the same
check for already mounted dispatch. Positioned operations use their explicit
offset and preserve the cursor. Sequential and conditional writes use the
cached cursor. Fill uses its bounded amount; prefix/head operations start at
zero. Overflow is rejected. A zero-byte observation does not intersect a lock.

Checksum/search and structural operations retain conservative whole-file
exclusion because their logical extent requires verified EOF or can move
subsequent bytes. Rename/swap also checks a separately locked destination;
source ownership alone cannot authorize replacing that object.

The selected COW volume's foreign-descriptor generation lease is unchanged:
even a disjoint write cannot invalidate another process's live volume-wide
snapshot. Close and process retirement still release locks without media I/O.
The subsequent [selected-path sharing extension](M1_UNRELATED_PATH_READER_SHARING.md)
permits unrelated path mutations only with an exact live-reader/generation
witness; descriptor mutation routes retain the conservative volume-wide lease.
These are bounded nonblocking single-BSP locks, not blocking POSIX locks or SMP
synchronization. Full per-file snapshot identity is separate work.

Four new regressions exercise dispatched bounded reads, positioned/fill write
rollback, destination locking and selected-media admission. They check overlap
denial, disjoint success, unchanged buffers/cursors/checkpoints/write budgets,
forged ownership, preserved selected-volume leases and lock release. An offline
device panics on any capacity/read/write access, proving contention denial
precedes those operations. Tests were authored but not run locally per the
user's instruction; final-head GitHub host/no_std and QEMU checks remain
required. This implementation alone does not accept G6 or G7.
