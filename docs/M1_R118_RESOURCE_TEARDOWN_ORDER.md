# R118 — Kernel resource teardown and mount admission order

This is an **enforced single-BSP ordering contract**, **not** proof of multicore lock safety.

## Authorized caller and lifetime

The scheduler must supply the currently authenticated `Identity {pid, generation}`. Neither user-supplied PID nor an untrusted timestamp grants authority. The kernel supervisor holds exclusive mutable access to the relevant tables during retirement. The same process incarnation must be checked in each table independently; no cross-service pointer may be reused after revocation.

## Lock-free (single-BSP) teardown order

1. **Event publication:** enqueue the generation-bound exit event exactly once while the original file identity is still live. A stale retry must not enqueue a replacement identity's exit.
2. **Network:** retire bound local UDP endpoints and the exact `Registry` grants together. The batch preflights **every** PID/generation against socket and registry state before mutating either. Orphaned grants are also withdrawn. Other PID/epoch datagrams remain queued.
3. **Delegated capabilities:** `LeaseTable::revoke_identity` for one process; for a multi-owner transaction use `revoke_identity_batch_kernel` to stage the full set and reject stale generations or duplicate PIDs before committing. A failing batch preserves all peer grants.
4. **Filesystem:** `FileHandles::retire_process_batch_kernel` preflights complete owner identities, then revokes handles/root authority. Any alias minted in a retired generation becomes stale, even when the same numeric PID is rebound.
5. **Memory/page tables:** after authority is no longer live, fallible page reclamation may be retried without resurrecting filesystem/network/capability authority.

Avoid acquiring these services in reverse order or calling into a different service from within an unfinished mutation. These are existing exclusive `&mut` Rust accesses, not nested spinlock acquisitions. The one-BSP scheduler still needs explicit synchronization before enabling physical APs/IRQs concurrently.

## Selected-volume admission order

1. Authenticate current owner generation and the selected-volume namespace guard **before** disk I/O.
2. Reject device-capacity mismatch and mount revision exhaustion without reading sectors.
3. Perform complete full MBR/GPT identity verification, mount health and filesystem checks.
4. Revalidate the originally acquired namespace, partition receipt, mount revision, and quarantine status after the probe.
5. Rotate the volume authority only on successful validation and publish the last-admitted namespace receipt. No stale alias or previously opened handle can implicitly select a new medium.

On an uncertain mutating disk error, quarantine the service and revoke cached handles until a privileged exact-receipt healthy remount proves recovery.

## Known limits and release gates

- There is **no inter-CPU lock** in these procedures. Real SMP/interrupt interaction, IPI cancellation, and physical NIC IRQ ownership need a separate lock-order analysis and AP test.
- The capability multi-owner API is currently a kernel primitive, not proof every scheduler rollback path invokes it atomically across all subsystems.
- Network ingress is a QEMU-backed tested path, not physical NIC/IOMMU DMA qualification.
- Physical boot-sector restoration, hardware clock calibration, immutable bootloader measurement root and real unsafe power-cut recovery remain unverified.
- Source-level tests and GitHub CI on the **final exact commit** are mandatory; do not merge based on a prior head's checks.

## Acceptance checks
- Replay old generation after PID rebind: new owner and peer lease/ports/handles must survive.
- An invalid member in a retirement batch must leave all peers unchanged.
- Mount revision overflow or parent-disk geometry mismatch must perform no selected-volume sector read.
- Successful mount must revoke prior aliases and retain monotonic service budget and revision.
- Event FIFO cleanup must preserve surviving generation-bound receipts, even at sequence exhaustion.
