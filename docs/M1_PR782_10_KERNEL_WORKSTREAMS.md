# PR #782 — ten coordinated M1 kernel-first workstreams

Parent PR head at start: `fa54b2d1c5caafb5782558861d033f1a61fc398d`. This is an additional integration commit
on the existing draft PR, **not ten independent agents** or separate PRs.

| Track | Subsystem | Implemented change | Host acceptance |
| --- | --- | --- | --- |
| WS01 | AHCI DMA | Reject null DMA buffers, checked span arithmetic for previous PRDT descriptors | Reject zero address and overflowing span; accept valid scatter/gather |
| WS02 | NVMe | Reject wrapping minimum 512-byte/block DMA span and expose epoch-scoped outstanding count | Wrap, submit, complete, stale epoch |
| WS03 | VirtIO | Strict metadata-only split-ring size and chain entry point | Reject 0/non-power-of-two/oversized queue sizes; accept size 1 |
| WS04 | ACPI RSDP | Reject reserved RSDP revision 1 before extended-length reads | Reject rev1 and preserve rev0 |
| WS05 | Crash log | Bounded, generation-filtered sequence cursor with no partial copy on capacity failure | Filter other owners, stale generation, undersized output |
| WS06 | Boot HHDM | Validate callback HHDM geometry and entire mapped page before returning a pointer | Reject low-half/overflow, accept canonical mapped page |
| WS07 | Filesystem ACL | Owner/root-only validated mode modification helper | Reject group impersonation, forged mode and forbidden mode bits |
| WS08 | PCIe ECAM | Width-aware numeric register address validation | Reject wrong sizes, unaligned and boundary-crossing accesses |
| WS09 | Paging map | Treat any overlapping reserved/partial boot memory region as veto | Allowed, reserved, partial overlap and malformed region |
| WS10 | PS/2 AUX | Route ACK ownership checks through the real injected transport logic | Primary channel never consumed, parity faults rejected, ACK/RESEND distinguished |

## Truthful scope and gates

These are host-testable production-code hardening slices and tests, not proof
of physical-device DMA isolation, real SMP TLB shootdown, signed firmware boot,
full compositor, or crash-proof persistent storage. Existing high-level M1
gates remain open until independently verified end-to-end evidence is available.

## Required exact-head acceptance

- [ ] `cargo test --manifest-path rust/kernel_core/Cargo.toml`
- [ ] Freestanding no_std release build, security, paging and kernel CI
- [ ] Real QEMU desktop + keyboard/mouse input, disposable-disk two-cold-boot persistence
- [ ] Two-vCPU relevant tests, independent review, protected merge, then main SHA validation

Do not mark these items checked based solely on source changes or a prior head SHA.
