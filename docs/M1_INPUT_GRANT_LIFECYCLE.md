# M1 input grant lifetime binding

`DesktopInput` previously remembered only `(owner, window)` for focus and
pointer capture. Withdrawing and recreating that pair, or assigning a window
away and back, made the old cached authority valid again. Existing broker
revocation correctly removed old queued events but could not invalidate that
cached authority.

Each successful grant or reassignment now receives a private checked `u64`
epoch from its broker. Desktop focus and capture retain the complete binding;
keyboard, click, and capture acquisition require the current binding to match.
A mismatch denies input and routing clears stale state. A trusted successful
focus operation explicitly acquires the new binding. `FocusTarget`, events,
and their wire encoding retain their existing public shape.

Epoch exhaustion denies new grants and reassignments before changing ownership
or queued events. Failed grant batches discard provisional epochs; successful
batches publish the updated counter with the bindings. Revocation never resets
the counter. This remains serialized single-broker kernel input context, as
required by the existing API; no global registry or SMP synchronization is added.

Regression coverage exercises revoke/regrant and reassignment away/back for
both keyboard and captured pointer routing, foreign and stale capture attempts,
peer queue preservation, successful fresh focus/capture, full-queue focus
atomicity, and epoch exhaustion including transactional batches.

Validation: all 43 `gui_broker` tests pass with offline locked Cargo. The
resurrection regression fails against the original broker source (stale capture
returns `Ok(())` rather than `Unauthorized`). An offline locked check with
`--no-default-features --target x86_64-unknown-none` passes. Existing unrelated
crate warnings remain.
