# Native desktop kernel trap-stack guards

The native `normal-desktop` profile reserves a page-aligned, inaccessible lower
guard page before each of its four 128KiB ring3 trap stacks and before the
dedicated 16KiB double-fault IST stack. Guard storage is outside the usable stack
budget. TSS RSP0/IST1 still point to the exclusive usable stack tops. Legacy
profiles retain their previous stack geometry.

Before any task's kernel-half root is cloned, the single-BSP runtime removes
these five guard mappings from the original active kernel CR3 with interrupts
masked and invalidates each guard translation. Original 4KiB payload frames
remain kernel-reserved; they are not freed or exposed to an application. A 2MiB
or 1GiB leaf is split into private page tables before a single parent-entry
publication. Neighbor physical addresses, permissions and cache/PAT attributes
are retained; huge-page PAT bit12 becomes PTE PAT bit7 when splitting to 4KiB.

Source-root split tables are permanent boot infrastructure. They are installed
before the process rollback PMM baseline, survive bounded desktop restart and
are not released with task roots. Allocation/access failure leaves the current
mapping intact. If cleanup itself refuses to release a private table, the helper
returns its frame ownership; the runtime retains those frames until reboot and
refuses re-entry, rather than losing ownership or reusing uncertain frames.
Already-absent zero guard entries are idempotent on a verified restart.

Before publishing a ready process, the runtime verifies that all five guards
are absent in both the source root and the cloned root, and that every usable
stack page is present, writable, supervisor-only and NX. A missing usable page,
bad permission, unsafe interrupt state or failed installation prevents process
startup. The serial verification marker is emitted only after actual source
page-table checks succeed; subsequent process readiness additionally requires
the clone checks.

Stack permission verification uses a separate effective-permission walk, leaving
the historical `paging_lookup` ABI unchanged. RW and user access are intersected
across all paging levels; NX is accumulated from every ancestor and the leaf.
A writable PTE beneath a readonly PML4/PDPT/PD is therefore rejected before
process publication rather than faulting later with CR0.WP enabled. Regressions
cover each level, 4KiB/2MiB/1GiB leaves, effective ancestor NX/U/S restrictions,
missing entries and malformed large-leaf mappings.

This is single-BSP protection; it supplies no AP TLB shootdown and does not claim
SMP support. Static stack payloads remain reachable through the trusted physical
direct map. The guard protects ordinary downward stack addressing, not arbitrary
kernel writes or a large stack-pointer jump over an entire guard. The separate
double-fault IST supports fatal diagnostics if a kernel stack overflows; there
is no claim that an overflowing kernel syscall can safely resume or restart.

Host regressions cover 4KiB removal, 2MiB/1GiB splitting, PAT and neighbor
translation preservation, source-map atomicity on either allocation failure,
inaccessible table cleanup, retained ownership when cleanup fails, malformed
entries and TSS/layout geometry. The combined Desktop QEMU test inspects all five
guard addresses as unreadable and usable endpoints as readable in both the
actual PID4 root and the reclaimed kernel root; it records distinct actual CR3
values and resolves stack object bounds from the same booted ELF. Current
evidence is in `build/normal-desktop-artifacts/run-a72yw5fo/transient-debugger.log`.
Host suites, freestanding compilation and the optimized ISO build passed.
Intentional kernel-stack overflow/DF delivery remains unqualified. This source
change alone closes no high-level M1 gate.
