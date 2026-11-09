# Selected-volume mutation and open descriptors

The native desktop's syscall32 selected-volume path service refuses a mutation
while another process holds an open descriptor on that selected volume. This
covers create, overwrite, positioned write, unlink, mkdir, rmdir, file rename and
directory rename. Rejection occurs before ATA discovery or disk inspection,
without changing user buffers, cursors, write credits or descriptors. It returns
the existing `InvalidName` service error; the syscall ABI is unchanged.

The selected descriptor service (syscall33) applies the same admission to write,
truncate, append/edit/prepend, positioned and generation-conditional writes,
create, unlink, file rename and directory-tree rename. Authentication and access
rights are checked before the lease conflict; conflict rejection precedes even
device-capacity inspection. The already-mounted kernel dispatch repeats this
check so it cannot bypass admission through a different service entry point.
Read, seek, close and successful fsync do not publish a new volume generation
and remain available while readers are open. Fsync still requires actual write
authority and a real successful storage barrier.

The lease is deliberately volume-wide. The bounded COW filesystem currently
tracks a volume generation, rather than stable per-inode identities. A path
mutation previously revoked *every* descriptor, even another process's reader
of an unrelated file. Refusing that mutation preserves the reader's authority
instead of silently replacing it. Two processes can still read concurrently.
The caller's own descriptors do not block its path operation and are revoked
on successful mutation as before. Closing the foreign descriptor, retiring its
process, narrowing its namespace or rebinding its PID releases its lease. A
stale/foreign process cannot close another incarnation's descriptor.

This is cooperative, single-BSP admission under the existing file-service
permit, not an SMP mutex or POSIX advisory lock. It does not add file ownership,
persistent ACLs, blocking lock acquisition or general multi-writer support.
Descriptors held by the same process retain conservative generation invalidation
after that process's own writes. Unselected legacy volumes preserve their
existing behavior. Selected-volume mutations can be denied by a reader of an
unrelated file: until the filesystem has stable per-inode identities, its global
generation makes that conflict real. A caller must release its lease explicitly
before another process can mutate; there is no automatic retry or waiting queue.

Whole-session recovery can verify the exact admitted volume with
`revalidate_retired_selected_volume` only when every file-service process and
handle has retired, the receipt is unchanged and the mount is not quarantined.
It rechecks the healthy partition read-only, without rotating revisions,
resetting write quotas, formatting or accepting uncertain storage. Quarantined
media still requires the existing explicit privileged recovery path.

Tests exercise shared reads, unchanged media/output/budget on refused mutations,
authenticated close, namespace revocation, PID reuse, stale tokens, successful
rename/delete after lease release, and retired-volume recovery preconditions.
Descriptor regressions cover every mutating action, denial before any device
access, unchanged quota/cursors/output, successful retry after authenticated
close, unrelated-file protection, mounted-dispatch bypass rejection, fsync with
shared readers and process/namespace/PID retirement. Torn-write acceptance first
proves admission denial, then closes the foreign lease and verifies the actual
injected disk error still quarantines all remaining tokens.
