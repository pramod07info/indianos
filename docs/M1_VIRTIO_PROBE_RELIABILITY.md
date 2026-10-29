# Bounded device-sized legacy virtio queues

The previous legacy NIC driver required exactly 256 entries. Under the local
Debian trixie QEMU the actual legacy receive queue reports 1024 entries, so
the driver rejected configuration before the first Ethernet exchange. The
starting revision passed networking under GitHub's runner; this was a real
supported-emulator variation, not a packet-parser failure.

The driver now reads each legacy queue size before allocation and calculates
descriptor, available and page-aligned used-ring offsets from it. Only nonzero
power-of-two sizes up to 1024 are accepted. Ring allocations are bounded to
eight contiguous pages per queue (three pages for size 256), and the entire
ring span must fit the existing checked direct-map/physical-address policy.
Configuration rechecks the device size before publishing the PFN.

The driver still submits only descriptor zero with one outstanding buffer per
queue. Exact completion progression, malformed-ID rejection and fault latching
remain intact. No offloads, mergeable buffers, interrupts, arbitrary devices or
general-purpose hardware DMA permissions are enabled. This remains a polling,
single-BSP QEMU-only legacy driver; queue sizes above the bound fail closed.

Regressions verify all accepted power-of-two layouts, invalid sizes, descriptor
and available/used ring separation, maximum allocation bounds, and a 1024-entry
ring's last slot plus u16 index wrap with an untouched allocation sentinel.
All eight driver tests passed. The real local QEMU ARP/ICMP/UDP/DNS/TCP exchange
now passes and host-side wire checks confirm each protocol stage.

```sh
cargo test --locked --offline --manifest-path rust/kernel_core/Cargo.toml \
  --features test-r028-network virtio_net::tests
make test-r028-network-qemu
```

Temporary diagnostic traces used to establish the reported queue size are not
part of the committed driver. The combined integration revalidates the final
driver together with the UDP endpoint changes and all other accepted fixes.
