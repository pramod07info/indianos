# Complete PMM boot-map validation

The Rust PMM validates all firmware entries before applying its 8 GiB bitmap
capacity limit. Usable ranges may not overlap any other entry, including
sub-page overlaps removed by inward page alignment. Zero-length, overflowing
and unreadable entries fail initialization. A rejected reinitialization leaves
the allocator disabled with zero managed frames.

The reader must provide an immutable trusted boot map. Initialization remains
single-CPU with interrupts disabled; this does not introduce SMP support.
Validation is bounded by 4096 entries and retains the existing quadratic overlap
check. Frames beyond capacity are ignored only after the entire map validates.

Regression: capacity-exhausting maps with invalid tails, reserved overlaps,
sub-page conflicts, adjacent ranges, and failed reinitialization.
Run: `cargo test --manifest-path rust/kernel_core/Cargo.toml --features pmm pmm::tests`.
Also run `make test-rust-pmm-host` and the required QEMU boot checks.

# Observed validation, 2026-10-29

Full host suite with `--features pmm`: 1,798 passed. After final test cleanup,
the four PMM tests passed again. `make test-rust-pmm-host` passed the original
C ABI harness. `make test-boot` passed QEMU/OVMF serial boot. Targeted
`no_std` x86_64-unknown-none check passed. Existing compiler warnings remain.

