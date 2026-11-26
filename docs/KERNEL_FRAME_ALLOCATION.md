# Fallible compositor frame allocation

Implementation task: M1-P2-004 (missing-code), first allocation-policy slice.

The normal desktop previously reserved 10,240,000 bytes of permanent kernel BSS
for frame staging. `kernel_buffer::Buffer` now allocates only the requested
frame size from the existing PMM, rounded to 4096-byte physical pages. A 1280 by
800 frame charges 4,096,000 bytes. A compositor epoch owns at most one buffer,
with a hard 10,240,000-byte limit including page rounding. Repeated frames reuse
it. The PMM buffer entry point permits up to 2500 pages, while legacy DMA
entry points retain their existing 16-page limit. Both share the same bitmap,
linear first-fit scan and atomic run-release validation. Metadata lives outside the payload; no user pointer controls PMM release.

Frame BEGIN validates authority, geometry and snapshots before allocation. OOM
refuses BEGIN without advancing its token or changing the visible framebuffer.
Failed resize invalidates any earlier pending transaction. Staged data becomes
visible only through the existing complete-frame COMMIT. Compositor retirement
revokes the transaction and scrubs/releases staging before replacement; the
physical framebuffer and client canvases remain intact. Full-session reset also
releases staging. A refused release retains ownership metadata and fails closed.

This uses the existing single-BSP, IRQ-disabled GUI/PMM contract. Allocation and
release reject enabled interrupts. It introduces neither a global allocator nor
SMP-safe heap access. Contiguous allocation can fail through fragmentation even
with sufficient aggregate free RAM. Such failure preserves the previous visible
frame; it does not promise successful desktop startup under memory pressure.

## Validation

The existing normal-desktop GitHub Actions job runs the new buffer arithmetic,
page quota, ownership, reuse/scrub, release-failure, resize/OOM and GUI publication
regressions. Its freestanding build exercises the real PMM/HHDM backend. The live
QEMU acceptance now requires `IndianOS: kernel frame buffer allocated` together
with PID4 full-frame presentation; existing compositor replacement and full-session
recovery scenarios exercise release/reallocation. Release diagnostics use
`IndianOS: kernel frame buffer released`. Host physical-memory fixtures stay
separate from the production backend and share the same ownership implementation.

No local build, test binary or QEMU execution was performed for this change,
per the owner's request. Exact-head GitHub results must be checked before merge;
adding tests alone does not establish a pass or gate acceptance.

## Remaining allocation work

M1-P2-004 remains incomplete: general-purpose allocation, multiple independent
kernel consumers, noncontiguous virtual arenas, allocation observability and SMP
synchronization still need implementation. This compositor-specific byte quota
is not a process RSS quota. System-wide OOM policy, process memory quotas, demand
paging and pressure recovery remain separate pending work in the canonical audit.
No milestone checkbox or estimated completion percentage changes in this slice.

## Initial GitHub failure and correction

Candidate `2a7e94f` passed host and freestanding checks, but live normal-desktop
job 139808535314 in run 45790659953 failed before readiness (PID4 exit 251).
The existing DMA allocator intentionally rejected requests exceeding 16 pages;
the first production frame exceeded that bound. The correction factors a shared
bounded run allocator/release path with a separate 2500-page kernel-buffer limit,
retaining the legacy DMA cap. Actual PMM bitmap regressions cover the maximum
buffer, exhaustion, fragmentation, duplicate release and failure atomicity.
Subsequent exact-head GitHub results are required; the failed candidate is not
acceptance evidence.

## Initial owned page heap: merged PR #821 (M1-P2-004)

`kernel_heap::try_allocate` adds a fallible, page-backed kernel allocation API
using the same PMM/Buffer backend. It accepts nonzero `Layout`s with alignment up
to 4096 and returns a unique lease with byte/word access. Allocations are zeroed;
release and Drop scrub them before returning pages. Sixteen external metadata
slots bound the total number of outstanding allocations. Generation tokens do
not wrap or permit a stale release to free a reused slot.

Quotas sum **all** live or quarantined physical page charges for the same owner,
not just the latest allocation. Current trusted owner classes are compositor
staging (10,240,000 bytes per epoch) and compositor syscall scratch (8192 bytes
per epoch). PID4 cannot choose these classes or quota limits through a syscall.
The 6400-byte row-copy scratch formerly on the syscall stack is now a distinct
owned allocation. It is reused across rows, preventing allocation/PMM scans on
every row, and remains private from the frame staging payload. Both owners retire
before replacement readiness; full-session reset also releases them.

Release failure revokes the lease first, retains the exact backing allocation
and owner charge, and quarantines its metadata slot. Bounded retirement cleanup
can retry these releases. A stale handle cannot retry or reclaim somebody else's
memory. Dropping a lease outside the IRQ-disabled contract retains ownership
until reboot rather than entering the single-BSP PMM unsafely. This is a kernel
programming error, not a supported asynchronous allocation/free path.

Host regressions cover aggregate page rounding/quotas, independent owners,
metadata exhaustion, stale and duplicate frees, generation exhaustion, OOM
atomicity, failed-release quarantine/retry, alignment, private payloads, Drop,
maximum-size frame plus scratch, scratch OOM and reuse. They exercise the same
heap engine with a separate page-aligned host RAM backend. Existing live desktop
acceptance additionally requires scratch allocation and retirement receipts.
GitHub Actions validates the exact PR head; no local tests/builds are run.

The first work item is **still incomplete**. This is a bounded page heap with two
production consumers, not a global Rust allocator or small-object arena. Remaining:
sub-page allocation/coalescing, noncontiguous virtual backing, additional kernel
owners, allocation-pressure diagnostics, SMP-safe admission, process RSS quotas
and system-wide OOM/recovery policy. No gate or percentage is advanced.


## Shared small-object scratch arenas (next M1-P2-004 slice)

The current allocator extends the PR #821 engine rather than adding another
physical allocator. `Owner::Frame` retains page-granular capacity and alignment.
`Owner::Scratch` now owns an 8192-byte arena and packs individual leases at
8-byte granularity, respecting requested alignments up to 4096. Physical quota
counts each backing arena exactly once, including quarantined arenas. A small
lease cannot resize beyond its own reserved extent into a neighbor's bytes.

There are at most 16 live leases and 16 backing regions. Metadata is external to
the payload. A bounded first-fit search uses live extents as its authority;
removing adjacent extents merges their holes implicitly. No in-band free-list
links or allocations for allocator metadata are needed. Allocation zeroes the
reserved extent. Free scrubs only that extent while neighbors remain alive;
only the final lease releases the arena. Failed final release quarantines the
entire region and keeps its physical quota charge until bounded reclamation
succeeds. Existing generation checks still reject stale/double frees.

Production use: native compositor BEGIN/ROW/COMMIT/ABORT descriptors use a
128-byte owned scratch lease. ROW's retained 6400-byte buffer shares that same
owner arena. After the first row, descriptor allocation/free reuses arena holes
without allocating physical pages. All user-pointer validation, frame generation
checks and complete-frame publication rules remain in force. Descriptors drop
before syscall return; the retained row lease retires with the compositor.

The serial receipt `IndianOS: kernel heap descriptor shares scratch arena` is
emitted once per arena after a successful ROW and only if its descriptor and
another lease are simultaneously live in that actual backing region. The live
normal-desktop and PID4-recovery harnesses require this receipt in addition to
frame/scratch allocation, publication and retirement evidence. Host negative
controls reject absent, duplicated or premature sharing receipts. This is an
allocation-path witness, not proof of general hardware or complete OOM policy.

New host regressions cover adjacent-hole coalescing without additional physical
allocation, nonadjacent fragmentation, alignment-padding reuse, live-neighbor
preservation on release, quarantine retry, metadata exhaustion without extra
backing, and once-per-arena sharing receipts. Existing quota, alignment, stale
handle, OOM, byte access, private payload and Drop tests remain applicable and
are updated for arena-based charging. No local builds/tests/QEMU are executed;
GitHub Actions results on the exact candidate are required before merge.

The first item remains unfinished: noncontiguous virtual backing, broader kernel
owners/global allocator integration, allocation-pressure diagnostics, SMP-safe
admission, process RSS quotas and system-wide OOM/recovery policy still require
implementation and end-to-end acceptance. This change addresses small-object
allocation/coalescing only; it does not advance a gate or completion percentage.
