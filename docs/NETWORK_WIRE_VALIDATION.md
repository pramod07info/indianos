# Strict QEMU packet validation

The existing live R028 single-peer packet parser now rejects IPv4's reserved
fragment flag, nonzero ICMP echo-reply code, TCP reserved/NS bits and contradictory
SYN/FIN/RST combinations. TCP option parsing checks bounded TLV lengths, EOL
padding and known MSS/window-scale/SACK/timestamp shapes before exposing payload.
Unknown correctly framed options are ignored. The non-ECN demo requires exact
SYN+ACK and PSH+ACK flags at its handshake and response stages, so extra FIN or
unnegotiated flags cannot masquerade as success.

Checksums remain mandatory before TCP options are interpreted. No DHCP, routing,
retransmission, general sockets or internet-ready TCP implementation is added.
This retains the fixed peer and bounded one-exchange QEMU scope.

Regressions generate incoming wire packets with recomputed valid IP/TCP checksums
and malformed flags/options, check every packet truncation, and accept valid
MSS/window-scale/SACK-permitted and unknown bounded options.
Run `cargo test --manifest-path rust/kernel_core/Cargo.toml network::tests`;
real device acceptance is `make test-r028-network-qemu`.

# Observed validation, 2026-10-29

Default host suite: 1,796 passed. Targeted `no_std` x86_64-unknown-none
check with test-r028-network passed. `make test-r028-network-qemu` FAILED
during the unchanged virtio PCI/DMA probe, before protocol exchange.
Unmodified main d137c217 reproduces that failure under the same Debian
trixie emulator/container. No end-to-end networking success is claimed;
passing full CI and device acceptance remain required before merge.

