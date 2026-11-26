# Fallible compositor frame allocation

Implementation task: M1-P2-004; related pressure/quotas: M1-P2-011.

## Current completion boundary

Source status below is not an exact-head CI pass or formal gate acceptance.
PR #825 contains the current follow-up; its required checks must pass before merge.

| Requirement | Current implementation |
| --- | --- |
| Fallible owned frame/scratch allocation, physical quota and stale-handle protection | Implemented; merged foundations in #819/#821 |
| Scratch small-object allocation and adjacent-hole coalescing | Implemented in merged #823 |
| Bounded quarantine reclamation/retry and pressure diagnostics | Implemented in #825; current-head acceptance pending |
| Enforced BSP, IRQ and non-reentrant metadata admission | Implemented in #825; rejects AP allocation, does not provide SMP allocation |
| Heap extent/backing integrity before unsafe operations | Implemented in #825; current-head acceptance pending |
| User-image data/stack admission budgets and retained failed rollback ownership | Implemented in #825, including existing legacy C parity; not total RSS |
| Noncontiguous physical backing behind contiguous virtual arenas | Not implemented |
| Broader kernel consumers/global Rust allocator integration | Not implemented |
| Allocator operation across multiple CPUs and corresponding PMM synchronization | Not implemented |
| Private launch footprint including image and kernel-clone tables | Implemented in #825: 512-frame admission and post-load consistency checks; guest acceptance pending |
| Total process RSS accounting/quotas including sharing and growth | Not implemented |
| System-wide OOM victim/recovery policy | Not implemented |
| Current-head guest pressure injection, multicore/hardware pressure acceptance | Not verified |

**The first allocation/OOM item is not 100% complete.** A passing host test or
commit cannot close the unimplemented or unverified requirements above. Subsequent
sections document implementation details and historical changes; this table is
the current remaining-work summary. No milestone checkbox or percentage changes.


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

## Initial PR #819 limitations (historical)

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

## Bounded quarantine recovery and pressure diagnostics

`try_allocate` uses `Heap::allocate_recovering`: a quota, metadata or physical OOM
refusal can trigger one pass over at most 16 quarantined regions. A successful
physical release authorizes at most one retry. No quarantine means no pass;
no successful release means no retry. Validation, ownership, mapping and release
errors do not trigger pressure retry. Live leases are never revoked or scrubbed.
Persistent refusal still reaches the existing caller's bounded fallback policy.

Retirement and pressure recovery share reclamation. Physical quota remains
charged until release succeeds. Saturating counters record requests, attempts,
final failures, cleanup passes, retries, recovered requests, reclaimed bytes and
peak physical backing. Snapshots distinguish live reserved extents, live leases,
total/quarantined backing and the requesting owner's backing charge. A recovered
first attempt counts as two attempts and zero failed requests.

Failure diagnostics reuse the runtime's allocation-free hexadecimal writer.
They report owner kind/epoch and memory counters without raw memory addresses.
Reason codes: 1 geometry, 2 quota, 3 physical OOM, 4 mapping, 5 release,
6 ownership/admission, 7 metadata/generation capacity, 8 alignment, 9 stale lease.
Each class is reported once, followed by reports at power-of-two failure counts;
alternating classes cannot cause one report per request indefinitely. Successful
recovery emits `IndianOS: heap allocation recovered after quarantine reclaim`.
These are heap counters, not total PMM or process memory statistics.

Host regressions cover recovered quota and global OOM, persistent OOM with one
retry, a single refused cleanup pass, validation rejection, live-neighbor bytes,
stale-token rejection, counter saturation and alternating-error report bounds.
They use a separate host failure backend and do not prove live guest OOM or
physical memory-pressure acceptance. No local builds/tests/QEMU run; exact-head
GitHub checks remain required before owner merge.

## Enforced heap CPU admission

Normal boot binds an independent heap instance of the existing non-spinning
`RetirementGate` immediately after PMM self-test. It reuses the existing x2APIC/
CPUID identity helper; no second CPU discovery or locking implementation exists.
Every production heap metadata/backend access acquires that exact gate before
creating its mutable reference. Unbound, wrong-CPU, IRQ-enabled and nested entry
are rejected. The permit releases on return; there is no spin while an interrupted
owner holds the gate. Rebinding to another CPU is refused. Frame/scratch creation
and retirement check admission before accessing their retained lease cells.

Admissions rejected before table access cannot safely increment heap counters or
read a diagnostic snapshot; they return an ownership/admission error directly.
A lease released outside its supported admission context keeps tracked ownership
rather than touching PMM unsafely. Supported allocation/free callers stay on the
registered BSP with IRQs masked; this is enforced serialization, not an SMP
allocator, per-CPU cache or support for allocating in an IRQ handler.

Host negative controls use the same guard wrapper to prove that wrong-core,
IRQ-enabled, unbound and reentrant calls do not execute their protected closure.
Nested entry through the real heap adapter is refused before touching the host
RefCell. The normal-desktop QEMU harness requires the boot admission receipt and
rejects reinitializing it during compositor-only recovery. Physical multicore
qualification and broader concurrent PMM use remain separate requirements.

## User-image staging rollback ownership

`user_image::stage` now records each newly allocated data frame before calling
fallible access/mapping callbacks. An unmapped frame whose rollback release
fails remains in the existing ownership list with virtual address zero. The
retryable image destructor already understands that representation. Successful
rollback clears the reserved entry and restores the previous page count. A
failed rollback stays charged against the fixed 128-data-page image capacity.
No UserImage layout or public C ABI changes are needed.

The previous error paths ignored a refused release and lost the only record of
the data frame. Regressions cover access failure plus refused release, conflicting
mapping plus refused release, successful rollback with no extra metadata charge,
and image-capacity rejection before another allocation. They verify eventual
release without double frees using the existing host backend. This is ownership
conservation on staging failure, not general process RSS quotas, demand paging
or a system-wide OOM killer.

## Extent and backing integrity before unsafe operations

Heap operations now verify their bounded external metadata before allocation,
release, sharing receipts or quarantine reclamation. Checks include valid owner
charges and generations; aligned, nonempty in-bounds extents; disjoint live
leases; disjoint physical and virtual backing ranges; and absence of live leases
in quarantine. Invalid metadata returns `Integrity` (diagnostic reason 10) before
scrub or backend allocation/release. It is never treated as retryable pressure.
Diagnostic summation of invalid live extents saturates rather than overflowing.

Corruption regressions deliberately inject overflowing offsets, invalid region
indices/generations, overlapping leases/backings and live quarantine. They verify
that allocator/backend calls and neighboring bytes remain untouched, then restore
metadata to verify ordinary cleanup. This bounds checks allocator metadata; it
is not a claim that arbitrary kernel memory corruption is recoverable. Unsupported
corruption stays fail-closed instead of guessing which physical pages are owned.

## Image data/stack admission budgets

`indianos_user_image_load_with_limit` is an additive trusted-kernel C API. It
accepts a 1..128 data-page limit and checks ELF/BSS plus stack requirements with
checked arithmetic before creating the root or invoking allocation callbacks.
The existing loader delegates with its original 128-page ceiling; UserImage
layout and existing ABI signatures remain unchanged. Failed preflight leaves
image/backend ownership untouched, and refused staging rollback still counts
against the image's fixed ownership capacity.

Normal desktop now applies an explicit 64-page data/stack admission limit to
PID1, which has no app pixel canvas; Files, Terminal and compositor retain 128.
Unknown desktop roles are refused. Existing read-only inspection of the earlier
unchanged desktop-init ELF showed seven segment pages; with its 16-page stack
this fits the 64-page limit. No image was rebuilt or executed locally. Fresh
exact-head guest acceptance remains required. The normal-desktop harness requires
`IndianOS: desktop broker image limit 64 pages enforced` and rejects repeating
it during compositor-only recovery, where PID1 must remain alive.

Rust preflight tests cover exact/exceeded budgets, invalid limits, reversed
ranges and excessive segment counts. Existing C loader integration now checks
that over-budget rejection invokes zero allocation callbacks, exact-limit loading
succeeds, live-image reload refuses, cleanup frees ownership, and the legacy API
continues to work. The desktop role policy has a separate negative-control test.

This admission policy covers image data/BSS and stack frames, not page tables,
shared mappings, kernel service buffers, runtime growth or total RSS. It does not
replace the still-pending process RSS/system-wide OOM requirements.

The legacy C loader is still built by existing parity/host checks. Its existing
implementation now exposes the same budgeted entry point and preserves staging
rollback ownership with retryable teardown, rather than leaving the additive
header unresolved in C builds. The shared C integration fixture also injects a
refused access/release and verifies retained ownership followed by clean retry;
it exercises both Rust and legacy C implementations in their existing CI targets.


## Private process launch footprint, including page tables

The normal desktop admits each process against a 512-frame private footprint
before loader/clone allocation. `required_private_frames` reuses the ELF planner
and counts data/BSS/stack frames plus the detached root and distinct PML4/PDPT/PD
prefixes needed by its 4-KiB mappings. Pages sharing table prefixes are counted
once; unmapped guard holes allocate no extra data. The existing 64/128 data-page
policy is independently enforced.

`paging::required_table_frames` performs a bounded read-only walk matching the
existing kernel-half clone traversal, including its private root and child tables,
but excluding borrowed data and huge leaves. It does not allocate, map, release,
switch CR3 or change the clone ownership policy. Malformed/unreadable tables or
excessive traversal fail admission. Checked addition combines this clone count
with the detached-image footprint; over-budget processes are refused before any
new image/clone frames are requested. After staging and cloning, actual image
page count and table traversal plus CloneResult.table_count must match the
preflight and fit the same budget before process readiness.

The normal-desktop QEMU harness requires the PID1 receipt `IndianOS: desktop
private frame budget 512 enforced` and forbids repeating it during compositor-only
recovery. Host regressions check table-count/clone parity without allocation,
unreadable/invalid entries, shared prefixes, guard-hole exclusion, exact budget
bounds and overflow. No local build/test execution or guest pass is claimed.

This is private launch-resource admission, not total RSS enforcement. Borrowed
kernel data, shared mappings, service-owned heap arenas and runtime growth are
separate resource domains. There is no general memory-growth syscall or shared
memory lifecycle here; future implementations must charge those resources under
an explicit policy. Noncontiguous backing, cross-CPU allocation and system-wide
OOM/victim recovery remain unfinished requirements.
