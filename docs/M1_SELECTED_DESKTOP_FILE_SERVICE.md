# Selected-volume desktop filesystem ingress

The native file service can now admit an explicitly selected, already formatted
partition before issuing any process identity. `prepare_selected_volume_kernel`
accepts a complete validated `PartitionReceipt`, verifies a strict healthy mount
without writes, and pins its mount revision. Admission rejects a table that has
ever issued a process identity, including one whose processes were retired.
It cannot replace an active volume or reset token, namespace or write budgets.

The boot/runtime caller must first match the configured GPT disk GUID, partition
GUID, ordinal and sector range, then obtain the complete receipt. A GUID is not
a hardware trust root. Receipt checks detect observed media changes; callers
still serialize and quiesce the device for each operation.

`operation_authenticated` now routes syscall33 operations through the pinned
selected-volume service when admitted. The ordinary `normal-desktop` profile
refuses the former 128-sector scratch adapter when no volume was admitted. That
legacy adapter remains available only to the existing experimental profiles.

`selected_path_operation_authenticated` supplies existing syscall32 operations
1–11 and syscall34 directory reads on the same selected volume: mkdir/rmdir,
create/read/overwrite/unlink, file/directory rename, enumeration and ranged I/O.
These retain the existing pathname ABI; actions10/11 mean range read/write at
this ingress, not the privileged `HierVfs::dispatch` copy variants. Namespace,
process generation, protected paths and PID2 read-only `/shared` restrictions
are checked before ATA probing. A denied request cannot query device capacity.
Read output is staged privately and published only after complete success.

Successful path mutations charge the same service-wide write guard as handles
and revoke existing descriptors rather than letting an old pathname descriptor
follow a renamed or replaced object. Data writes charge their byte count;
structural mutations charge one operation and one unit. The existing bounded
32-operation/4096-byte service budget is unchanged. Failed mounts or uncertain
I/O quarantine the volume and retire cached descriptors; closing or rebinding
does not recover that authority.

`selected_sync_authenticated` is PID1-only and invokes the existing filesystem
sync and actual block-device barrier on the selected partition. An uncertain
barrier/verification result quarantines the service. Successful shutdown must
observe this result before claiming persistent completion.

Regressions use actual selected-sector COW/VFS operations for admission, failed
receipt retry, retired-process admission denial, CRUD/range/remount, stale and
foreign owners, namespace escape, quota exhaustion before I/O, changed media
without output publication, and real barrier failure quarantine. The coordinator
must execute these with the combined runtime and real GPT QEMU profile; host
tests alone do not establish live syscall, compositor or physical acceptance.

This remains a serialized single-BSP service. General user/group ACLs, concurrent
mount replacement, persistent quotas, an authenticated physical device identity,
and comprehensive power-loss/hardware qualification remain unfinished.
