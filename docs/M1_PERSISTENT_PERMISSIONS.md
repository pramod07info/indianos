# Persistent discretionary permissions

Filesystem metadata v2 uses the six previously reserved bytes in each used
40-byte COW entry for u16 owner, u16 group and owner/group/other mode bits.
These values are included in the existing metadata CRC and published in the
same checkpoint as the object. Unsupported modes are rejected even with valid
checksums. Overwrite and rename preserve the object's ownership and mode.

V1 volumes remain readable. Their original unrestricted discretionary semantics
are represented explicitly as owner0/group0/mode0777; existing namespace and
S0 restrictions still apply. V2 publication occurs on credential-aware create
or chmod, without formatting or repairing the disk. Older kernels cannot read
v2 permission metadata; downgrading such a volume is unsupported.

Credentials are issued inside the kernel, never accepted from a syscall UID
field. The existing native system broker defaults to uid0; the Files service
defaults to uid1000. Trusted kernel admission may assign a process another
uid/group and revokes its handles first. A new process incarnation resets the
role default; stale incarnations cannot assign credentials or use old handles.
This does not implement login, session authentication or supplementary groups.

Authenticated descriptor and selected path operations check directory search,
data read/write and parent mutation permissions before accessing file data.
Metadata-atomic creates include the credentials and mode from the outset.
Root credentials bypass discretionary bits but cannot bypass namespace roots,
protected paths, service role restrictions or write budgets. Credential changes
never grant new namespace roots. Cached close/seek/unlock remain possible without
reading file contents; physical read/write operations still validate the mount.

Native syscall33 operation38 changes mode on a current descriptor: the argument
is at most0777; path/data fields must be empty. The process must retain the
system broker role and be root or the file owner. The original volume snapshot,
namespace, foreign-reader exclusion and S0 write budget are checked. Success
requires a real flush, charges the budget and retires handles affected by the
metadata generation change. Uncertain storage failures quarantine selected
volumes. Shutdown refuses this mutation after durable shutdown arming.
ABI query37 now advertises featurebit2 for persisted mode permissions.

Tests were added for owner/group/other distinctions, invalid modes with valid
CRC, overwrite/remount persistence, ancestor search and parent-write denial,
native chmod, stale handles, credential replacement and rebinding. They have
not been run locally; exact-head GitHub compilation/tests and QEMU remain
required. These are bounded POSIX-style discretionary permissions, not named-user
extended ACLs, a general multiuser filesystem or verified hardware durability.


## Native ownership administration

Syscall 33 operations **39 (change owner)** and **40 (change group)** reuse the
same 64-byte descriptor as mode operation 38. The existing ABI query operation
37 advertises the additive ownership-administration bit **256**; version 1 and
size 64 are unchanged. The handle is at bytes 24..32 and the new u16 owner/group
ID is encoded as a u64 at 32..40. IDs above 65535 reject rather than truncate.
Path, data pointer, data length and reserved fields must be zero.

Only the current PID1 broker may invoke these operations. Owner transfer
requires a kernel-issued root credential before probing media. Group changes
require root or the persisted file owner selecting its own kernel-issued group.
The selected mount receipt, namespace, current file generation, S0 policy and
write budget remain mandatory. Outstanding foreign descriptors reject before
I/O; root cannot override this snapshot exclusion. A successful change flushes
and revokes cached descriptors; failed I/O retires selected volume authority
without charging a successful write. Durable-shutdown arming rejects both
operations. Userspace cannot use the target ID as caller credentials.

The trusted VFS entrypoints also support directory and symlink metadata without
following links. Native operations remain descriptor-based file administration;
no directory-handle or login/account-management API is added. Named-user ACL
entries, supplementary groups, persistent quotas and general multiuser
administration remain unfinished. Host tests cover admission without device
access, persisted publication, token revocation and failed flush. Exact-head
GitHub validation is required; no local tests or hardware qualification ran.


## Bounded named-user ACLs (metadata v3)

Version 3 retains the eight existing 40-byte file entries and places two
`(u16 uid, u16 rights)` records per object in bytes 352..416 of each checksummed
metadata sector. Remaining tail bytes must be zero. Active UIDs are nonzero,
unique and ascending, followed by zero records. Rights use READ=4, WRITE=2,
SEARCH=1; a zero-rights active record is an explicit deny. Orphan records on an
unused object, duplicate UIDs, unknown rights and noncanonical padding reject
the volume even when its CRC is correct. Versions 1 and 2 remain readable with
empty named ACLs. Setting the first real ACL publishes v3 atomically; ordinary
writes, durable writes, rename and chmod/chown/chgrp preserve ACLs and never
downgrade the format. Older kernels cannot mount v3: downgrade is unsupported.

After validating all metadata, root and the owner retain their existing mode
semantics. A matching named user receives its named rights intersected with
the mode's group bits, with no fallback to group/other on denial. Nonmatching
users keep the existing group/other policy. This bounded mask convention is
not a full POSIX ACL: there are no named groups, default/inherited ACLs or
supplementary groups. Directory ancestors require SEARCH through this same
policy; no ACL grants a wider namespace or implicit symlink traversal.

Syscall33 operation **41** sets a named user: argument bits0..15 hold UID,
bits16..31 hold rights (0..7), and upper bits must be zero. Operation **42**
removes a nonzero u16 UID. Both reuse the existing zero-path/zero-payload native
64-byte descriptor and advertise feature bit **512**. These are operation
numbers within syscall33; the separate syscall41 remains the blocking-lock
ABI. PID1's broker role is mandatory, and VFS permits ACL changes only to a
kernel-authenticated root or current owner. Foreign descriptors block metadata
administration before device access. Success flushes and revokes cached
handles; uncertain I/O quarantines the volume. Shutdown arming rejects changes.

Host tests cover canonical policy, mask/deny precedence, remount/write/rename
preservation, ancestor search, CRC-valid malformed records, torn publication,
pre-I/O denial, descriptor revocation and failed flush. The separate guest
ownership fixture additionally grants a named uid1000 read, verifies exact
committed bytes, applies explicit deny, removes the grant and verifies denial
again. Its oracle requires twelve real metadata publications, final empty ACLs,
restored root:root/0777 and unchanged payload/guard sectors. Product images do
not include that fixture. No local builds/tests ran; exact-head GitHub evidence
is required. Persistent quotas and general multiwriter locking remain pending.


## Persistent owner quotas (metadata v4)

Version 4 keeps the v3 ACLs and adds four quota records in bytes416..448 of
each checksummed metadata sector: u16 UID, u16 maximum object count, u32 maximum
logical payload bytes. Active UIDs are nonzero, unique and ascending, followed
by zero records; unused tail bytes remain zero. Limits are bounded by the
existing eight objects and 16384 total logical bytes. A nonzero UID with zero
limits is a real deny-all quota. UID0 remains unquotaed; only a trusted root
credential may set/remove limits. V1/v2/v3 mount without quota records. Setting
a limit publishes v4; subsequent ACL/mode/data operations preserve that version.
Downgrading v4 to older kernels is unsupported.

Usage is derived from each committed object's owner, counting each object and
its stored length exactly once. Directories and symlinks count as objects and
charge their marker/target bytes. Quotas are per owner, not per writer, and do
not measure physical bank reservation or page cache. Growth/create validation
precedes data-bank writes in both ordinary and flush-ordered paths. The shared
metadata publication barrier also checks quota on ownership transfer and every
other mutation. Shrink/delete reclaim derived usage atomically; a failed or
interrupted mutation never resets a separate cached counter. Limits below
current committed usage reject. CRC-valid malformed or over-quota metadata
refuses mount rather than fabricating accounting.

Native syscall33 operations43/44 set/remove a volume quota through a current
selected descriptor, advertised by featurebit1024. SET argument has UID in
bits0..15, object limit in16..31 and byte limit in32..63; REMOVE uses only a
nonzero u16 UID. Only PID1 with root credentials and an unrestricted `/`
namespace may administer a volume quota. Existing foreign-reader exclusion,
write budget, flush-before-acknowledgement, token revocation, quarantine and
shutdown policy remain in force. No user-supplied ID becomes caller identity.

## Ordered range-lock admission

The live BSP coordinator now issues monotonic nonzero tickets for contended
waits. Current older conflicting receipts take precedence over younger blocking
and nonblocking acquisitions. Disjoint ranges and shared/shared requests remain
independent. Stale/revoked namespace, handle and volume receipts are excluded
from ordering. Cancellation/timeout does not acquire a lock or discard an old
held lock; ticket exhaustion rejects new waits without reusing old authority.
The existing deadline, range checks and lock table are reused, not duplicated.
This remains bounded cooperative locking for the current trusted roles, not
arbitrary threads, POSIX lock queues or SMP synchronization.

Host tests cover quota persistence, zero limits, data-write preflight, owner
transfer, reclaim, corrupted metadata, torn removal and guarded native admin;
lock tests cover ordered handoff, blocked deadlines and stale receipts. The
separate guest proves a persisted zero quota rejects ownership transfer before
removal, plus younger-reader timeout and nonblocking no-barging before writer
handoff. Its oracle checks fourteen COW publications, final empty ACL/quotas,
restored root permissions and unchanged payload/guard sectors. No local build
or test ran; exact-head GitHub evidence remains required.
