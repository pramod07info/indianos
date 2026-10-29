# Retryable detached VMM table ownership

The Rust detached VMM retains ownership links until the backend accepts a
page-table frame release. Previously map rollback and unmap pruning cleared
the parent entry first: a refused release stranded an allocated frame outside
the address-space tree.

Map rollback now stops at the first refused release, retaining that table and
its ancestors. Unmap returns success once the requested leaf has been removed;
optional pruning refusal leaves empty table frames reachable and owned by the
space. The caller still owns the unmapped data frame.

`vmm_destroy_empty` first verifies the complete tree contains no mappings. It
then reclaims retained empty subtrees, unlinking only accepted releases. A
refused child or root release leaves remaining ownership reachable and permits
a later destroy retry. A still-live sibling prevents all destruction before
any table is released. The existing C ABI layouts and functions are unchanged.

The backend must return distinct, accessible table frames and retain ownership
when its release callback returns false. Operations remain serialized,
non-reentrant and limited to detached roots; no active CR3, TLB invalidation,
SMP locking, or ownership of leaf data frames is introduced.

## Validation

The feature-enabled VMM suite passed all five tests, including three new
regressions: refused unmap pruning at each level, failed map rollback with
refused cleanup, and whole-tree destruction preflight/partial cleanup/root
release retry. The tests verify all allocated table frames remain reachable,
no mapped data frame is released, and retries reclaim every table exactly once.

Commands:

```sh
cargo test --locked --offline --manifest-path rust/kernel_core/Cargo.toml --features vmm vmm::tests
cargo check --locked --offline --manifest-path rust/kernel_core/Cargo.toml --no-default-features --target x86_64-unknown-none --features vmm
```

Original C ABI parity and combined user-image/process QEMU acceptance are run
by the integration coordinator; they are not claimed as independently observed
by this task.
