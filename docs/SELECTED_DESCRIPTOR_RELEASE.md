# Selected-volume descriptor release

SEEK (handle operation 4) and CLOSE (operation 5) now use the authenticated
in-memory descriptor table on an already admitted selected volume. Neither
operation probes ATA, queries device capacity, reads partition metadata,
remounts a filesystem, flushes storage nor changes its COW generation.

This fixes a cleanup dependency: an unavailable disk previously prevented a
reader from closing its descriptor. That retained the conservative volume-wide
read lease and blocked another process's writes even after storage returned.
Closing the authenticated reader now releases that exclusion independently of
device availability. The runtime adapter and explicit selected-device API use
the same admission code, as does already-mounted selected-volume dispatch.

The current process incarnation, namespace root, live token and admitted volume
must still match. The explicit selected-device API also requires the supplied
receipt to equal the cached selected receipt. Empty path/buffer fields are
mandatory; CLOSE requires a zero argument and SEEK retains the existing maximum
file-size bound. Foreign, stale, malformed and quarantined requests fail before
any media inspection or descriptor change. A seek changes only the cursor: it
does not refresh a stale filesystem generation or acquire another file object.

CLOSE is capability release, not fsync. Callers requiring durability must use
the existing authenticated flush barrier before closing. Reads, writes and
fsync retain their existing physical receipt, mount health and payload checks.
Quarantine and quota state are preserved; closing cannot re-admit uncertain
storage or reset write budgets.

Added regression source uses a block device that panics on **every** capacity,
read or write call to prove offline seek/close admission and foreign-lease
release. It also exercises the production runtime adapter before native ATA
probing, malformed descriptors, wrong receipt/PID/generation, and stale COW
identity after a cursor change. No local build or test execution was performed,
as requested. These host tests do not establish hot-unplug hardware support.
