# Fallible compositor frame allocation

Implementation task: M1-P2-004 (missing-code), first allocation-policy slice.

The normal desktop previously reserved 10,240,000 bytes of permanent kernel BSS
for frame staging. `kernel_buffer::Buffer` now allocates only the requested
frame size from the existing PMM, rounded to 4096-byte physical pages. A 1280 by
800 frame charges 4,096,000 bytes. A compositor epoch owns at most one buffer,
with a hard 10,240,000-byte limit including page rounding. Repeated frames reuse
it. Metadata lives outside the payload; no user pointer controls PMM release.

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
