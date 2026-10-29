# M1 PCI bridge routing bounds

The read-only HD1 scanner previously checked each bridge independently. A
bridge on bus 1 assigned buses 2–4 could therefore be followed even when its
ancestor assigned only buses 1–3. Two bridges on the same upstream bus with
ranges 1–5 and 3–7 also passed the distinct-secondary check.

Each queued secondary bus now retains its enclosing subordinate ceiling and
its parent queue index. Configured bridges must satisfy
`primary == current bus < secondary <= subordinate <= enclosing ceiling`.
The increasing bus order and inherited ceiling establish containment in every
ancestor window transitively. The root ceiling is 255. Inclusive routed ranges
of bridges sharing a parent bus must be disjoint; nested windows are valid and
are not mistaken for sibling overlap. Checks run before enqueueing a destination.
Malformed ordering or containment returns `MalformedBridge`; ambiguous sibling
routing returns `AmbiguousBus`.

The existing `secondary == 0` unconfigured-bridge skip precedes these checks.
The scanner remains allocator-free, visits at most 32 buses, examines 32 slots
per bus and up to eight functions only when function zero advertises them,
and preserves the existing accelerator limit. Added queue metadata is bounded
by the same 32-entry constant. No config writes, BAR sizing, mapping, DMA or
optional driver activation was added. Errors still propagate through the same
`Result`; the existing `kernel_main.rs` error arm continues CPU-only boot,
reports unknown GPU/NPU and unavailable QPU, and skips optional device probing.
That caller was inspected and was not modified.

Validation on the task branch:

- `cargo test --locked --offline --manifest-path rust/kernel_core/Cargo.toml hardware_pcie`:
  12 passed, including seven existing HD1 tests and five new routing tests.
- `cargo check --locked --offline --manifest-path rust/kernel_core/Cargo.toml --no-default-features --target x86_64-unknown-none`:
  passed. Existing unrelated warnings remain.

Both commands used `CARGO_HOME=/workspace/.cargo`,
`RUSTUP_HOME=/workspace/.rustup` and `/workspace/.cargo/bin` on `PATH`.
New sparse provider fixtures cover direct and deep ancestor escapes, backwards
secondary ordering without a duplicate bus, sibling intersections in both
orders and at an inclusive endpoint, nested sibling overlap, valid adjacent
siblings, three-level topology, multifunction bridges and GPU/NPU endpoints.
Per-bus read counters prove rejected destinations were never probed. They also
verify the original 32-bus cap and bridge-register provider error propagation.

Limitations: this validates firmware-reported segment-zero topology reached
from bus 0; it cannot authenticate firmware or discover hidden devices. Only
secondary buses explicitly exposed by bridges are enumerated, not every bus
number in a routed window. PCIe ECAM activation, other root buses/segments,
hot-plug and device drivers remain outside scope. The tests use mock config
providers, not physical PCI hardware. Combined QEMU bridged-GPU acceptance is
owned by the integration coordinator and is not claimed by this change.
