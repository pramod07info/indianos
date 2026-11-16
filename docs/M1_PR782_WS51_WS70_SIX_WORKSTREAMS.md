# PR #782 — WS51–WS70 (six kernel-first workstreams)

## Scope

Twenty **implemented no-alloc/no_std Rust source-level operations** across six
existing kernel modules, with negative and positive host regression tests in
each module. One additional integration commit on the *existing* PR branch.

Six workstreams are logical code ownership groups, **not six physically
running autonomous agents**. No new PR or independent branches.

| ID | Workstream | Implementation | Positive / negative acceptance |
|---|---|---|---|
| WS51 | Storage / TinyFS | Atomic bounded directory name listing | live names in order; short buffer unchanged |
| WS52 | Storage / TinyFS | Validated metadata size lookup | correct length; missing name rejected |
| WS53 | Storage / TinyFS | Strict range read with after-EOF tail validation | correct bytes; bad range/corrupt sector leaves output unchanged |
| WS54 | Storage / TinyFS | Single-metadata-sector rename | data survives; destination collision rejected |
| WS55 | Capabilities | Monotonic in-place rights attenuation | subset succeeds; expansion/peer request denied |
| WS56 | Capabilities | Authenticated rights-subset delegation with new token | fresh recipient ticket; escalation denied |
| WS57 | Capabilities | Batch owner/resource authorization | all tickets must pass; no granted partial operation |
| WS58 | Capabilities | Atomic same-owner ticket revocation | forged, repeated, peer tickets preserve all grants |
| WS59 | Process events | Owner-filtered receipt snapshot | FIFO order; small destination unchanged |
| WS60 | Process events | Exact owner+head acknowledgement | peer/replayed receipt cannot dequeue |
| WS61 | Process events | Bounded per-PID sequence cursor page | peer isolation, no event consumption |
| WS62 | SMP | Verify exact live pending AP lifecycle publication | stale, mismatched and acknowledged proof denied |
| WS63 | SMP | Two-fence acknowledged reclaim admission | either missing/forged AP acknowledgement blocks |
| WS64 | SMP | Publish successor only after exact earlier ack | no premature reuse of generation |
| WS65 | NIC RX | Trusted hardware-quiesced *and* software-drained check | ongoing permit or missing HW proof blocks |
| WS66 | NIC RX | Checked generation rebind / IRQ resume boundary | no new RX before both HW flags and drain |
| WS67 | NIC RX | Device/instance/epoch receipt tied to live permit | foreign, stale, quiesced receipts fail |
| WS68 | VFS path | Atomic parent+basename extraction | malformed or short buffer leaves both untouched |
| WS69 | VFS path | Validate both rename operands inside trusted tree | sibling-prefix / traversal / separator alias denied |
| WS70 | VFS path | Bounded canonical basename replacement | parent preserved; malicious child/short output denied |

## Restrictions and integration

- Filesystem helpers are for **legacy one-sector TinyFS**; rename metadata
  sector writes are **not power-loss durable** and TinyFS is **not journaled**.
- Capability, event and VFS helpers require authenticated caller context,
  separate filesystem locking and ACL evaluation, and kernel-owned buffers.
- SMP acknowledgements are software fences only; no IPI/TLB shootdown or
  multi-core scheduler integration is added by these helpers.
- NIC driver caller MUST derive \`irqs_masked\`/\`dma_stopped\` from trusted
  hardware state. Passing user-provided booleans is unsafe.
- These functions are implemented in the **actual kernel core modules**, but
  callers must explicitly integrate the stricter APIs; their presence does
  not certify complete hardware gates.
- No claims of completed physical DMA/IOMMU, verified boot, power-loss
  recovery, real SMP shootdown or protected release certification.

## Verification checklist (must be exact pushed SHA)

- [ ] Host \`cargo test --manifest-path rust/kernel_core/Cargo.toml --lib\`
- [ ] Freestanding no_std and C/Rust ABI build checks
- [ ] Security gate, kernel boot and QEMU regression workflows
- [ ] Persistent desktop and multi-vCPU/boot-epoch workflows
- [ ] Independent review / protected main merge and post-merge SHA audit

Record CI results separately after push; do not check boxes on source-code
authorship alone.
