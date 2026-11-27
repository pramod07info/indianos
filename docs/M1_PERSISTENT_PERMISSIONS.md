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
