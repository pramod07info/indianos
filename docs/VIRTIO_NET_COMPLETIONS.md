# Legacy virtio-net completion ownership

The opt-in legacy QEMU NIC submits only descriptor zero, with at most one
outstanding buffer per queue. Completion now requires exactly one offered
entry and exactly one step of used-index progress, including u16 wraparound.
Invalid descriptor IDs, oversized lengths, index jumps, rewinds and unsolicited
completions fail without advancing the driver's used counter. RX/TX faults
latch the device closed; no further copy, submission or reuse is allowed.

The compiler acquire fence follows observation of the used index. Hardware
ordering still relies on x86 coherent DMA. This remains a polling, single-BSP,
IRQ-masked QEMU-only driver; it does not add physical-device qualification,
IOMMU isolation or general multi-descriptor support.

Host regressions exercise the actual Queue against aligned memory-backed vrings,
including wraparound and faulted RX with unchanged output and no I/O-port access.
Run: `cargo test --manifest-path rust/kernel_core/Cargo.toml --features test-r028-network virtio_net::tests`.
Device acceptance: `make test-r028-network-qemu`.

# Observed validation, 2026-10-29

Host suite with `--features test-r028-network`: 1,800 passed. Targeted
`no_std` x86_64-unknown-none check passed. `make test-r028-network-qemu`
FAILED during the PCI/DMA probe, before any packet exchange. The identical
command on unmodified main d137c217 also failed at that stage with the same
Debian trixie emulator/container. This is not passing device acceptance;
the draft must remain unmerged pending passing full CI and device validation.

