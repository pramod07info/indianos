# Retry-safe detached clone release

`paging_clone_release` previously released a parent even when a descendant
release failed, and left links to successfully released children. Its false
return retained `CloneResult.root`; retrying could traverse freed frames or
release siblings twice. This is a prerequisite for runtime abort cleanup retries.

Detached clone release now clears a child link only after that whole child
subtree is released. If any child fails validation, access or release, its
ancestors remain owned and accessible. Other successfully released sibling links
are cleared. A parent is released only once every owned descendant is gone.
`CloneResult.root` remains nonzero on failure and is zeroed after complete release;
`table_count` retains its original allocation count until successful completion.
Mapped payload frames and huge-page leaf mappings are borrowed, never released.
The caller must restore the kernel CR3 and detach borrowed user children before
release. This helper does not perform either operation or add SMP locking.

Three added regressions exercise release refusal at each of eight frames in a
four-level branching tree, repeated refusal followed by successful retry, access
failure at each level, leaf payload exclusion, and constructor allocation failure
at each of four levels. The test backend poisons freed tables and asserts against
access-after-free, source-frame release and duplicate release.

The existing clone-construction failure contract is preserved: when the release
backend works, allocation failures roll back all created tables and leave the
source and output result unchanged. A combined construction failure plus refusing
release backend still has the preexisting best-effort constructor rollback
limitation: the constructor does not publish an incomplete clone. This change
makes release of a published owned clone retry-safe; it does not claim to resolve
that separate constructor ownership-transfer policy.

Standalone tests of the actual paging production module passed all four tests.
No QEMU or physical hardware gate is closed by these injected host failures;
normal kernel, scheduler and preemption QEMU regressions remain required at the
combined integration revision.

Whole-core `cargo test --offline --manifest-path rust/kernel_core/Cargo.toml
--features paging paging:: --lib` passed four tests; `cargo check --offline
--manifest-path rust/kernel_core/Cargo.toml --no-default-features --features
paging` passed with 72 existing warnings. No warnings or checks were suppressed.
