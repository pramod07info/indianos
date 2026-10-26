# R037 Batch 3 — Ten bounded hardware/security foundations

Development branch: `integration/r037-batch3-ten-tasks`.
Based on last green main R034 SHA `25626297f2a311815bff218036dd5301829f4df7`
because R035 Batch 1 PR #88 and R036 Batch 2 PR #89 are still unmerged.
All ten tasks are represented by one coordinated commit. This is **not**
a report of functioning physical drivers, a secure updater or a real TLS stack.

| Priority | File | Scoped implementation | Still missing before completion |
|---|---|---|---|
| 21 Driver isolation | `batch3_driver_isolation.rs` | Exclusive MMIO/IRQ leases, generation validation, fault quarantine | IOMMU, trusted IRQ dispatch, real driver process isolation |
| 22 AHCI | `batch3_ahci.rs` | Bounds/alignment/overlap checked SATA DMA command preflight | AHCI HBA enumeration, SATA link init, durable DMA, real disk tests |
| 23 NVMe | `batch3_nvme.rs` | Namespace bounds and command-ID lifecycle across controller resets | PCI BAR mappings, admin/I/O queues, PRP scatter/gather, interrupts |
| 24 VirtIO | `batch3_virtio.rs` | Cycle-safe checked split-ring chains and read/write directions | Device feature negotiation, safe memory mapping, real queue notifications |
| 25 ACPI | `batch3_acpi.rs` | RSDP and SDT headers with checksums, revision and length enforcement | ACPI table traversal, MADT/APIC routing and actual RSDP mapping |
| 26 DHCP/TCP/TLS | `batch3_network_security.rs` | Strict DHCP options, TCP handshake state and certificate verifier interface | DHCP packet/header authentication, full TCP reliability, real TLS engine/PKI |
| 27 Secure updates | `batch3_updates.rs` | Anti-downgrade signed update policy, A/B stage/attempt/health confirmation | Cryptographic signature verification, image hash, durable A/B disk, boot rollback |
| 28 Crash diagnostics | `batch3_crashlog.rs` | Bounded metadata-only crash ring with sequence and stable iteration | Persistent storage, watchdog capture, userspace crash reporter |
| 29 Hardware matrix | `batch3_hardware_matrix.rs` | Exact-SHA/platform-bound qualification bitmask; skipped/failed fail closed | Real physical machines and QEMU test attestations for each driver |
| 30 Security audit | `batch3_security_audit.rs` | Exact-SHA checks for seven control categories and independent review | Independent audit evidence, threat model, penetration testing and tooling |

`batch3_acceptance.rs` includes a cross-module safety test across device
grant, AHCI, NVMe, VirtIO, malformed ACPI, DHCP, TCP, staged update,
crash report, hardware qualification and review-gated security evidence.

## Merge/build plan

1. Run `cargo test --offline --manifest-path rust/kernel_core/Cargo.toml`
   and freestanding target `cargo build --offline --manifest-path
   rust/kernel_core/Cargo.toml --target x86_64-unknown-none
   --no-default-features`.
2. GitHub Actions mandatory `IndianOS kernel boot (x86-64)` must pass
   on this exact R037 commit SHA.
3. R035 Batch 1 must be independently reviewed and merged; its new
   main SHA must pass the post-merge kernel suite. Next R036 Batch 2 must
   integrate onto that main and pass again before its reviewed merge.
4. Recreate/rebase R037 single commit on **the final merged Batch 2 main**
   without erasing concurrent changes. Re-run mandatory kernel, smoke,
   source-audit and exact-SHA checks for the new commit.
5. Only after those checks pass and review requirements are satisfied,
   merge Batch 3 to main. Validate the new main build before Batch 4.

Security boundaries: no real disk formatting, privileged MMIO or unchecked
DMA; no arbitrary physical memory reads; no executable downloads; no
claim that CRC is a cryptographic signature; no disabled CI gates. Code here
is bounded Rust policy/validation scaffolding with tests.
