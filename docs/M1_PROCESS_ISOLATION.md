# Retryable user-image teardown

The existing user-image destructor revokes user-copy access before releasing
any resources. It now records progress per page: a zero virtual address means
the leaf has been unmapped but its data frame is still owned; a zero physical
address means that data frame has already been released. Real staged images
never use virtual page zero or physical frame zero.

If unmapping, data-frame release or root release fails, a later destructor call
retries only resources still owned. It neither re-unmaps a removed leaf nor
double-frees a released frame. An invalid page count fails before array access.
Successful final teardown resets the image metadata as before. C ABI layout
and successful lifecycle behavior are unchanged.

This relies on VMM unmap success meaning that the leaf is removed; optional
page-table pruning refusal retains reachable tables for destroy-time cleanup.
The VMM ownership fix in the same integration batch supplies that contract.
The caller still restores kernel CR3 and serializes teardown on the BSP. This
does not introduce SMP-safe reclamation or a new userspace ABI.

Fault-injection regressions cover data-release refusal, inaccessible root,
root-release refusal, failed user-copy after teardown begins, duplicate frees,
and invalid page counts. Focused host tests passed five cases. The coordinator
runs combined freestanding, original C ABI and QEMU lifecycle regressions on
the accepted integration revision.

```sh
cargo test --locked --offline --manifest-path rust/kernel_core/Cargo.toml \
  --features user-image user_image::tests
```
