# IndianOS kernel P0 Batch36 — 25 real source acceptance tasks, six validation workstations

Baseline: draft PR #788 at `6bb48e2794cc9621849684765d394c8e7e3ad32b`. One integrated commit on the existing branch; six CI lanes are **validation workstations**, not six independent coding agents. Never merge until all exact-head checks and independent review pass.

| Task | Lane | Acceptance criterion |
|---|---|---|
| P0-866 | W1 PMM | Report real free bitmap count through claim/release lifecycle |
| P0-867 | W1 PMM | Forged used-count never appears as free capacity |
| P0-868 | W1 PMM | Unaccounted occupancy fails closed, reconciled count works |
| P0-869 | W1 PMM | Invalid span/cursor and pre-init metadata disclose no free count |
| P0-870 | W2 APIC | Live two-sided APIC progress receipt accepted |
| P0-871 | W2 APIC | Epoch rearm and PIC takeover revoke progress |
| P0-872 | W2 APIC | Counter rewind and capture over-budget cannot authenticate |
| P0-873 | W2 APIC | Stop, exhaustion, saturation and zero-progress rejected |
| P0-874 | W3 ring-3 | Loaded writable+executable segment cannot grant read |
| P0-875 | W3 ring-3 | Loaded writable+executable segment cannot grant write |
| P0-876 | W3 ring-3 | Executable read-only segment remains readable, never writable |
| P0-877 | W3 ring-3 | Non-executable ELF data retains bounded read/write |
| P0-878 | W3 ring-3 | Empty staged numeric ABI remains compatible |
| P0-879 | W4 storage | Revision-pinned write reports success only after real sync |
| P0-880 | W4 storage | Failed sync reports partial writes, never false durability |
| P0-881 | W4 storage | Midwrite media replacement prevents sync/success |
| P0-882 | W4 storage | Stale revision/out-of-range selection touches no disk |
| P0-883 | W5 network | Layer-2/ARP sender consistency accepts requests and replies |
| P0-884 | W5 network | Ethernet-vs-ARP sender spoofing is rejected at ingress |
| P0-885 | W5 network | Zero/multicast ARP sender identity rejected |
| P0-886 | W5 network | Valid IPv4 unaffected, short ARP still rejected |
| P0-887 | W6 boot | Two distinct aligned framebuffer descriptors accepted |
| P0-888 | W6 boot | Duplicate descriptor pointer rejected before consumption |
| P0-889 | W6 boot | Misaligned late framebuffer descriptor rejected |
| P0-890 | W6 boot | Null, zero and excessive descriptor tables denied |

## Production changes

- `pmm.rs`: Free-frame diagnostic now verifies the exact live bitmap and span layout. Hot allocation path is unchanged.
- `timer.rs`: xAPIC progress diagnostic checks epoch/controller/IRQ/capture before and after publication.
- `syscall.rs`: Loaded ELF segment preflight refuses W+X memory even for reads; staged empty numeric plans still require independent PTE authorization.
- `block.rs`: New revision-pinned durable write API with explicit sync barrier, final media preflight and completed-sector failure receipt; no false rollback claims.
- `virtio_net.rs`: ARP sender hardware address must match the received Ethernet source MAC; valid existing fixtures now include sender identity.
- `boot.rs`: Reject duplicate framebuffer descriptor pointers across the full Limine response table.

## Verification and release restrictions

`p0-batch36-implemented` adds six exact-PR-head jobs with 4+4+5+4+4+4 = **25 named Rust tests**, plus six `no_std` x86-64 target checks. Existing full x86-64 kernel boot, M1 persistent desktop/VFS, R028 networking, R114 epoch, R126 AP, security and previous P0 tests are retained.

Source integration **does not equal verified execution**. Record the actual new SHA and GitHub CI results, keep draft/unmerged if anything fails. Full SMP/TLB shootdown, IOMMU/DMA isolation, destructive power-loss recovery, TPM and physical hardware production qualification remain OPEN.
