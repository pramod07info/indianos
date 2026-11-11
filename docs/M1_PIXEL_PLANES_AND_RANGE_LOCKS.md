# Retained pixel planes and filesystem range-lock ABI

This integration starts at main `df7118c161fa3ea1f4b3e2a78c8352ec0f342f29`.
It advances G3–G7; it does not close the complete M1 or hardware gates.

Normal Desktop now uses two real retained pixel planes for Files and Terminal.
Each boot-pinned allocation contains 768×768 encoded pixels and a coverage mask,
totalling 594 payload pages, bracketed by two unmapped guard pages. Repaint
rasterizes the client's bounded command stream into its own backing, then
composites covered pixels in the ring-3 manager's window order. Uncovered pixels
preserve chrome. Replacing commands rebuilds the backing so old glyphs cannot
replay. Retirement and session reset clear both pixels and coverage in place.

The runtime freezes the actual backing geometry before cloning task roots. It
requires all payload pages to remain supervisor-only RW/NX and all guards to
remain absent in source and cloned address spaces. The QEMU debugger checks
payload endpoints and guards in PID4 and recovery roots, and requires unchanged
addresses after restart. The original command-buffer guard checks remain.
No pixel allocation is copied on the kernel stack or moved across restart.

These planes are kernel-private, process-owned raster backing. Applications
still submit drawing commands; this does not implement application-mapped
pixel buffers, GPU acceleration or fully userspace raster composition. PID4
retains independent window policy, while the privileged backend draws pixels.

## Filesystem descriptor ABI v1

Syscall 33 retains its 64-byte descriptor and existing operations 1–6. Its
additional operations are authenticated through the current PCB incarnation:

| Operation | Behavior |
| --- | --- |
| 34 | Acquire/replace a shared byte-range lock on a readable handle. |
| 35 | Acquire/replace an exclusive range lock on a writable handle. |
| 36 | Release this handle's lock; argument must be zero. |
| 37 | Query ABI: version in bits0–15, descriptor bytes16–31, features32–63. |

For locks, descriptor `argument` packs a u32 start in its low half and u32
length in its high half. Zero length means through the maximum file extent.
Empty/overflowed ranges reject. Path and data pointer/length must be zero.
Acquisition returns 0 on success, 1 on contention without changing the prior
lock, and `u64::MAX` for invalid/unauthorized requests. ABI query requires
handle/argument/path/data zero; it reports version1, 64 bytes, feature bits0
(basic descriptors),1 (range locks) and, in the following permissions batch,
2 (persistent mode permissions). Unknown operation numbers reject.

Locks belong to descriptor tokens and kernel-issued process generations.
Close, retirement, root revocation and I/O token invalidation remove them.
Overlapping exclusive locks exclude peer reads and writes; shared locks permit
peer reads. Cached control/query operations do not probe storage. Shutdown
allows unlock/query but refuses acquiring new locks after durable sync arming.
Files exercises ABI discovery, shared lock/unlock and denied exclusive locking
on its real read-only native handle before displaying file contents.

This is bounded nonblocking locking on the existing single-BSP file service.
It does not add POSIX blocking/fair waits or SMP synchronization. The existing
conservative volume-generation writer exclusion remains: range locks do not
permit mutation behind another process's live snapshot. Named-user extended
ACLs and a general multiwriter filesystem remain unfinished.
The follow-up [persistent permissions](M1_PERSISTENT_PERMISSIONS.md) adds
kernel-issued credentials, stored owner/group/other bits and native chmod38.

## Validation boundary

Positive/negative tests were authored for pixel isolation, transparent coverage,
failed batches, clearing/replacement, guard geometry, lock contention,
disjoint reads, stale generations, ABI shape and native user-pointer bounds.
The normal QEMU harness requires real pixel-guard and native lock/ABI markers.
No local build, test or QEMU was executed, following the requested workflow.
Only exact-head GitHub Actions can validate the integrated source.

Four worker sessions started, but execution usage limits stopped them. The
coordinator completed this bounded pixel/lock integration directly. Unfinished
ACPI firmware/power code remains outside this PR. The permission format was
subsequently completed in the linked permissions batch; full extended ACLs
remain open. Physical multicore, NIC IRQ/DMA,
storage power-loss and ACPI qualification still require selected real hardware.
