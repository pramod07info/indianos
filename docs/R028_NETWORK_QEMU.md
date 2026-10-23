# R028 — Virtio-net PCI networking verified in QEMU

## Delivered and verified

This milestone adds a real, **opt-in single-core legacy virtio-net PCI**
device driver, an allocation-free network protocol client, and a
deterministic QEMU socket-backed virtual Ethernet peer. It does not use
the host's internet connection or require a VPN or LAN bridge.

| Layer | Verified behavior |
|---|---|
| PCI | Enumerate 0x1AF4:0x1000 legacy virtio-net PCI, configure I/O BAR and bus mastering |
| Device DMA | Negotiate MAC-only features; bound two polling virtqueues to 3 physically contiguous 4-KiB pages each; independent frame bounce buffers |
| Ethernet | Exchange real emulated guest/host MAC-addressed Ethernet frames |
| ARP | Resolve the host's 10.7.0.1 address with an actual ARP who-has and reply |
| IPv4/ICMP | Validate IPv4 length, checksum, destination and fragmentation policy; verify echo request/reply |
| UDP | Request/reply with pseudo-header UDP checksums, ports and bounded payload |
| DNS | Request `A indianos.test`, transaction 0x2828, validate the answer 10.7.0.1 |
| TCP | Single client three-way handshake, one validated request/response, final ACK; reject mismatched ACK, sequence or checksum |

The [R028 QEMU verification run #39933935521](https://github.com/pramod07info/indianos/actions/runs/39933935521)
passed 71 Rust tests, a freestanding `x86_64-unknown-none` build,
the active Rust source audit, and a real QEMU virtio-net run.
The **host-side peer** validated exactly one each of ARP, ICMP ping,
UDP echo, DNS A query, TCP SYN and TCP data, plus the final TCP ACK.
The guest independently emitted ordered success markers.

### Reproduction

```sh
cargo test --offline --manifest-path rust/kernel_core/Cargo.toml
make test-r028-network-qemu
```

The test requires QEMU, OVMF, Clang/LLD, xorriso and the Rust bare-metal
x86-64 target. It creates a localhost-only listening socket, uses QEMU
`-netdev socket` and `-device virtio-net-pci,disable-modern=on`, and
delivers/receives real Ethernet frames. The QEMU user-mode firmware
sometimes sends unsolicited IPv6 neighbor solicitation before the Rust
kernel starts; the host peer ignores only that unrelated pre-boot IPv6
multicast and requires its own expected IPv4 packets in order.

The feature flag `test-r028-network` is disabled by default. No
network hardware or DMA is touched by normal boots. The QEMU driver
must be operated with one bootstrap processor, interrupts masked,
a working PMM and Limine HHDM. The PCI device is explicitly reset
before its bounce-buffer DMA frames are reclaimed. No physical NIC
configuration is authorized.

## Important limits

R028 passes a **bounded network-client acceptance test**, not a
production or general-purpose protocol stack. It does not implement
DHCP, IPv6, TCP retransmission/flow/congestion control, arbitrary
connections, server sockets, packet fragmentation/reassembly, DNS
resolver cache, internet access, TLS, firewall, multi-queue virtio,
modern PCI virtio capabilities, PCIe topology enumeration, virtual
network isolation across processes, or hardware-network driver support.
The host peer deliberately avoids real internet access and keeps the
test reproducible.

Future networking work should add interrupt-backed modern virtio
queues, verified retransmission and sockets, network service isolation,
and secure user-space network APIs. No claims about a full internet
stack or broad hardware support should be inferred from this test.
