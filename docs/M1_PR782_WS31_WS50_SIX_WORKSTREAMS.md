# PR #782 — WS31–WS50: six kernel-hardening workstreams

## Scope and evidence rules

This batch extends the same open PR branch; 20 **source-level implemented**
kernel policy functions or operations, with corresponding host regression tests.
It does **not** implement physical DMA/IOMMU mapping, NVMe/AHCI MMIO,
APIC shootdown, genuine packet authentication, ACPI physical mapping,
persisted flush barriers, or protected main release proof. The external
trusted DMA/firmware/driver/transport authorities must be wired to these
fail-closed APIs before they certify a hardware gate.

These are six coordinated code workstreams, **not six independent running
agents**. All changes are staged in one integration commit.

| ID | Workstream | Source change | Acceptance scenario |
|---|---|---|---|
| WS31 | AHCI / storage | Media-epoch and read-only command preflight | stale epoch and write-to-readonly rejected before mapper |
| WS32 | AHCI / storage | PRDT exclusion against reserved hardware regions | partial overlap/overflow denied |
| WS33 | AHCI / storage | IOMMU per-4KiB-page ownership preflight | unowned second page denied |
| WS34 | AHCI / storage | exact-epoch write → flush → durable fence | false early/different-ticket acknowledgement rejected |
| WS35 | NVMe | checked page ownership before queue insertion | failure leaves outstanding count zero |
| WS36 | NVMe | full request-identity completion including block count | mismatched block count retains live CID |
| WS37 | NVMe | exact request abort by epoch and full identity | wrong DMA/stale epoch cannot cancel |
| WS38 | NVMe | quiescence-gated controller reset | rejected reset retains live commands |
| WS39 | VirtIO | disjoint chain with every pinned DMA page verified | second unowned descriptor denied |
| WS40 | VirtIO | negotiated maximum individual descriptor size | oversized segment denied |
| WS41 | VirtIO | epoch-bound wrapping used-ring cursor | zero progress, stale epoch and oversized batch denied |
| WS42 | ACPI | transactional mapped RSDT/XSDT address copy | failure preserves entire output |
| WS43 | ACPI | checked MADT variable record chain | malformed record sizes rejected |
| WS44 | ACPI | SDT within trusted mapped physical aperture | short/wrapped aperture rejected |
| WS45 | Network | DHCP gateway constrained to address's subnet | off-link and self-gateway rejected |
| WS46 | Network | BOOTP identity and exact DHCP ACK/server-ID checking | wrong transaction, malformed type denied |
| WS47 | Network | peer-bound TCP three-way handshake policy | wrong peer cannot establish |
| WS48 | Driver isolation | driver+epoch+IRQ exact quarantine | peer sharing wrong IRQ identity stays active |
| WS49 | Driver isolation | complete-page MMIO grant preflight | unaligned and overbound page denied |
| WS50 | Driver isolation | transactional quiesced rebind | conflict retains quarantined old grant |

## Integration/verification

- Production code lives in six existing modules, not in documentation-only
  placeholders: \`batch3_ahci.rs\`, \`batch3_nvme.rs\`,
  \`batch3_virtio.rs\`, \`batch3_acpi.rs\`,
  \`batch3_network_security.rs\`, \`batch3_driver_isolation.rs\`.
- Run: \`cargo test --manifest-path rust/kernel_core/Cargo.toml --lib\`
- Run freestanding no-std build and the branch's complete kernel/security/
  QEMU workflows. Report exact pushed head SHA and run conclusions.
- The preexisting permissive entry points are retained for compatibility;
  callers must opt into the stronger WS31–WS50 entry points. **Their addition
  alone is not proof that production device paths are protected by them.**
- Hardware and production acceptance gates remain open pending real devices,
  trusted lifecycle integration, independent review and exact-main SHA proof.
