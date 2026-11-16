# IndianOS M1 PR #782 — WS91–WS120 (30 tasks, six workstreams)

This batch implements **30 concrete no_std, allocation-free Rust kernel-core
operations**, five per workstream, with host-test regressions and a single
integration commit on the already-open PR #782. It does **not** start six
independent agents and does not create a new PR.

## WS91–WS95: SATA/AHCI storage
- WS91: \`plan_transfer\`: atomic multi-command split by max transfer bytes,
  checked LBA range and caller capacity.
- WS92: \`require_write_persistence\`: reject write-back configurations without
  independently verified FUA or flush support.
- WS93: \`enforce_prdt_boundary\`: no PRDT buffer crossing a 64KiB aperture.
- WS94: \`verify_completion\`: match successful read/write completion to exact
  epoch, ticket, sector count, LBA and transferred bytes.
- WS95: \`WriteFlushFence::begin_next\`: reject same/older flush ticket replay.

## WS96–WS100: NVMe queue admission
- WS96: \`Queue::submit_batch\`: atomic fixed-capacity multi-request submission
  with duplicate CID/namespace/LBA validation.
- WS97: \`Queue::complete_batch\`: transactional full-request completion.
- WS98: \`Queue::namespace_outstanding\`: epoch-scoped namespace count.
- WS99: \`Queue::snapshot_requests\`: no partial short-buffer copies.
- WS100: \`Queue::reset_drained\`: disallow silent reset with pending commands,
  require trusted controller hardware-stop evidence.

## WS101–WS105: VirtIO split-ring lifecycle
- WS101: \`validate_ring_layout\`: descriptor/available/used region alignment,
  non-overlap and numeric bounds.
- WS102: \`validate_available_heads\`: reject repeated/out-of-queue head IDs.
- WS103: \`validate_used_element\`: strict completion lengths and direction.
- WS104: \`UsedRingCursor::advance_budgeted\`: exact epoch and caller budget.
- WS105: \`validate_mixed_chain\`: checked read-header then write-response
  descriptors with disjoint memory and no unauthorized flags.

## WS106–WS110: ACPI/APIC and timer firmware
- WS106: \`parse_enabled_apics\`: enabled, unique LAPIC/x2APIC IDs.
- WS107: \`parse_ioapics\`: unique ID/GSI/MMIO base checks.
- WS108: \`hpet_mmio_address\`: checksum, bounded table and GAS checks.
- WS109: \`fadt_reset_io\`: checked numeric reset I/O address and value.
- WS110: \`madt_lapic_override\`: unique, aligned LAPIC override parsing.

## WS111–WS115: network ingress
- WS111: \`require_ipv4_unicast\`: deny multicast/reserved/broadcast peers.
- WS112: \`strict_ipv4_header\`: bounded fixed IPv4 frame and checksum, no
  fragments, bad TTL or unsupported protocol.
- WS113: \`strict_udp_datagram\`: exact nonzero destination, length/checksum
  presence and validated IPv4 envelope.
- WS114: \`bounded_retransmit_ms\`: saturating finite retry schedule without
  arithmetic overflow.
- WS115: \`renewal_schedule\`: T1 < T2 < expiry for validated DHCP leases.

## WS116–WS120: driver MMIO/IRQ ownership
- WS116: \`Drivers::snapshot_active\`: atomic active grant snapshot.
- WS117: \`Drivers::quarantine_batch\`: transactional exact-generation group
  quarantine with no partial state on forged/duplicate identifiers.
- WS118: \`Drivers::retire_batch_quiesced\`: transactional group retirement
  only after trusted external hardware quiescence.
- WS119: \`Drivers::authorize_irq\`: live driver/IRQ/generation identity.
- WS120: \`Drivers::authorize_mmio_outside_reserved\`: whole-page MMIO
  authorization excluding trusted firmware-reserved regions.

## Files and evidence

- \`rust/kernel_core/src/batch3_ahci.rs\`
- \`rust/kernel_core/src/batch3_nvme.rs\`
- \`rust/kernel_core/src/batch3_virtio.rs\`
- \`rust/kernel_core/src/batch3_acpi.rs\`
- \`rust/kernel_core/src/batch3_network_security.rs\`
- \`rust/kernel_core/src/batch3_driver_isolation.rs\`
- This task ledger under \`docs/\`.

All six modules contain source-level positive and negative host tests.
Exact-head verification must include host kernel-core Cargo tests, security
CI, combined no_std builds, QEMU kernel boot, desktop/persistent FS checks,
boot epoch evidence, two-vCPU checks and required independent review.

**This code does not** drive AHCI/NVMe/VirtIO devices, map arbitrary DMA,
perform APIC IPI or TLB shootdown, read bare-metal firmware, verify real
network authentication, perform persisted power-loss recovery, or certify any
M1 hardware gate. Some new hardened operations are opt-in until production
callers switch to them; do not mark a high-level gate complete merely because
these tests compile. No protected main-branch merge is claimed.
