# Bounded UDP datagram delivery and port teardown

The existing single-peer QEMU UDP ingress now retains valid zero-length
datagrams. An empty datagram consumes one queue slot, preserves FIFO order,
and returns `Ok(0)` from `receive`; `Err(Empty)` means that no datagram is
queued. A nonempty datagram still requires a sufficiently large output buffer
and is never truncated or consumed by a short read. Output bytes outside the
returned length remain untouched.

`UdpEndpoints::close(pid, port)` closes one authenticated binding and discards
only its unread datagrams. Other ports belonging to the same process and all
peer processes keep their bindings and FIFO contents. Rebinding the closed
port cannot inherit earlier packets. `close_all` preserves its existing API
and now uses the same stable, linear queue compaction.

`EpochUdpEndpoints::close(pid, generation, port)` requires the current
scheduler-authenticated generation. The owner record remains until the final
binding closes, preventing a recycled PID from mixing generations while an
older port remains open. Wrong-owner, stale-generation and already-closed
requests fail without mutation. A process-wide `retire` still closes every
binding for the matching generation.

The transport remains the existing fixed-peer, fixed-capacity QEMU demo:
Ethernet/IPv4 peer checks, UDP port/length checks, mandatory transport checksum,
the 64-byte payload cap and queue capacity remain enforced. This does not add
general sockets, routing, network authentication or an internet-ready stack.
Callers must derive identity from trusted kernel process state and serialize
access; these objects are not interrupt-safe or SMP-safe.

Regression tests construct real, checksummed incoming Ethernet/IPv4/UDP bytes
and deliver them through `ingest_ethernet_frame` before receiving them. They
cover empty messages mixed with nonempty payloads, full queues, short outputs,
per-port close/rebind, retained sibling/peer messages, generation turnover and
zero-port/zero-queue capacities.

```sh
cargo test --locked --offline --manifest-path rust/kernel_core/Cargo.toml \
  --features test-r028-network network::
cargo check --locked --offline --manifest-path rust/kernel_core/Cargo.toml \
  --no-default-features --target x86_64-unknown-none --features test-r028-network
```

Real NIC acceptance remains `make test-r028-network-qemu` on the combined
integration revision; host packet tests do not establish device operation.

Observed local validation: the focused network filter passed 71 tests with
the NIC feature enabled, and the freestanding check above passed. Existing
compiler warnings remain. No standalone QEMU success is claimed for this task.
