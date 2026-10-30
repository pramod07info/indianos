# Compositor damage survives only its original grant

The M1 bounded compositor queue previously checked numeric owner/window IDs and
current geometry at presentation. Revoking and regranting the same IDs, or
transferring a window away and back, could make old queued work appear valid.
Recreating the surface with identical dimensions had the same problem.

`DamageQueue::submit_verified` now captures the current broker grant epoch and
surface incarnation for every rectangle, after validating the entire batch.
`present_fair_verified` and `purge_unowned` require both epochs to remain current.
Surface grant and movement revisions use checked monotonic counters; exhausting
one cannot publish a grant or change geometry. Moving a window away and back
also invalidates previously queued work. Stale work is removed without changing
peer FIFO order. Coalescing refuses to merge different authority incarnations,
preventing stale work from acquiring a newer grant by merging with a fresh frame.

Public `FrameDamage` and `OwnedSurface` values and the graphics wire ABI remain
unchanged. Authority stays private to queued records. Existing trusted local
`submit`, `drain`, `preview_owner` and `present_fair` remain available; unbound
legacy submissions are deliberately rejected by verified presentation. Services
that require current authorization must pair `submit_verified` with
`present_fair_verified`, rather than bypassing verification through legacy drain.

Five regressions cover revoke/regrant and transfer-back ABA, fresh-frame and peer
preservation, same-size surface recreation, unbound legacy frames, failed batch
rollback, coalescing across generations, movement and counter exhaustion.
Existing R052 verified-presentation tests now use authenticated submission.

This is a single-BSP serialized kernel primitive, not an independent ring-3
compositor. The queue, broker and layout must retain the same instances during a
session; on service reconstruction the coordinator must discard the old queue.
Table-local epochs do not authenticate a separately reconstructed broker or
layout with restarted counters. No SMP, physical hardware or high-level M1 gate
completion is claimed by these host regressions. The live desktop retained-command
raster path is unchanged; its existing QEMU regression remains an integration
requirement for the coordinator.

Validation on this workstream: `cargo test --offline --manifest-path
rust/kernel_core/Cargo.toml gui_broker --lib` passed 52 tests (including five new
regressions); `cargo check --offline --manifest-path
rust/kernel_core/Cargo.toml --no-default-features` passed with 72 existing compiler
warnings. An independent rustc test harness importing the actual rectangle and
broker production modules passed all 69 tests. No warnings were suppressed.
