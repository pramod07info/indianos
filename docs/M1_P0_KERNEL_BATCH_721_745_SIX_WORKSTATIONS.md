# P0-721–745: IndianOS kernel-first six-workstation batch 29

> Source-level changes are added to existing draft PR #788. Six lanes are independent CI *validation* workstations, not autonomous developer agents.

## Implemented task ledger

### W1 PMM

- [x] P0-721: Atomic exact claims — source and regression added, exact-head CI PENDING
- [x] P0-722: Overlap denies aliased bitmap claim — source and regression added, exact-head CI PENDING
- [x] P0-723: Corrupt offset prevents release — source and regression added, exact-head CI PENDING
- [x] P0-724: Corrupt cursor prevents claim — source and regression added, exact-head CI PENDING
- [x] P0-725: Atomic release rejects double-free — source and regression added, exact-head CI PENDING

### W2 APIC

- [x] P0-726: Live delivery receipt — source and regression added, exact-head CI PENDING
- [x] P0-727: Concurrent new IRQ retained — source and regression added, exact-head CI PENDING
- [x] P0-728: Rewound IRQ counter denied — source and regression added, exact-head CI PENDING
- [x] P0-729: Stop, rearm and exhausted budget denied — source and regression added, exact-head CI PENDING

### W3 ELF

- [x] P0-730: Valid file offset accepted — source and regression added, exact-head CI PENDING
- [x] P0-731: Overflowing ELF offset denied — source and regression added, exact-head CI PENDING
- [x] P0-732: Boundary-safe ELF offset accepted — source and regression added, exact-head CI PENDING
- [x] P0-733: Corrupt ELF denied for user copy and IRET — source and regression added, exact-head CI PENDING

### W4 Storage

- [x] P0-734: Stable generation accepts read — source and regression added, exact-head CI PENDING
- [x] P0-735: Changed generation pre-read revokes — source and regression added, exact-head CI PENDING
- [x] P0-736: Mid-read generation change revokes — source and regression added, exact-head CI PENDING
- [x] P0-737: Restored generation cannot unretire — source and regression added, exact-head CI PENDING

### W5 Network

- [x] P0-738: ARP and IPv4 accepted — source and regression added, exact-head CI PENDING
- [x] P0-739: Unsupported IPv6 ingress denied — source and regression added, exact-head CI PENDING
- [x] P0-740: Unknown protocol egress denied — source and regression added, exact-head CI PENDING
- [x] P0-741: VLAN and 802.3 denied — source and regression added, exact-head CI PENDING

### W6 Boot

- [x] P0-742: Canonical low-half pointer — source and regression added, exact-head CI PENDING
- [x] P0-743: Canonical high-half pointer — source and regression added, exact-head CI PENDING
- [x] P0-744: Noncanonical pointer denied — source and regression added, exact-head CI PENDING
- [x] P0-745: Null/misaligned pointer denied — source and regression added, exact-head CI PENDING

## Source changes

- PMM exact-run claims/releases now refuse corrupt span geometry before bitmap mutation.
- APIC ring-3 capture fences a non-rewinding hardware IRQ delivery sample.
- ELF numeric and strict execution policy rejects file offset + length overflow.
- Stable read-only media window pins media_generation and revokes on changed capacity/generation, even if restored.
- Legacy virtio R028 filters unsupported Ethernet-II protocols; IPv4/ARP remain supported.
- Limine pointer validation rejects noncanonical x86-64 four-level addresses before Rust reference creation.

## Acceptance (pending exact head)

- All 6 new CI jobs pass with 5+4+4+4+4+4 = 25 **named** Rust regressions.
- Every new lane passes `x86_64-unknown-none` freestanding build.
- All inherited QEMU, kernel boot, VFS, network, PMM, security and M1 workflows pass the SAME SHA.
- Independent review, branch protection and hardware qualification remain separate. Keep PR draft until required checks.

## Explicitly open: hardware SMP/IPI/TLB, DMA/IOMMU lifetime, destructive power-loss replay, live PTE authorization, TPM verified boot and physical hardware release tests.
