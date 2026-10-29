# M1 boot buffer span validation

`boot.rs` now validates the complete `pitch * height` framebuffer byte span
before status rendering, console exposure, or desktop color access. Checked
multiplication rejects size overflow. The shared arithmetic validator rejects
null/empty spans, invalid alignment, lengths exceeding `isize::MAX`, exclusive
end overflow, noncanonical first/last bytes, and spans crossing the four-level
x86-64 canonical hole. Framebuffers require four-byte alignment; module bytes
require byte alignment. The existing width/pitch and nonoverlapping RGB mask
checks remain in force.

`boot_initramfs_file` applies the same validator after its existing 16-byte to
32-MiB module-size bound and before the initramfs parser can create a raw slice.
Malformed modules return false without publishing outputs. Malformed
framebuffers retain the serial-only fallback (console views return `None`).
Ordinary low-half host buffers and Limine higher-half mappings remain accepted.
The last accessed byte may end at a canonical-half boundary; an exclusive end
that wraps is conservatively rejected.

These checks prove arithmetic properties, not page-table mapping, allocation
provenance, or exclusive access. Limine response structures and their referenced
storage must remain valid and mapped until boot consumers finish. No memory-map
reclamation or mapping policy is changed. The kernel uses four-level paging;
this intentionally does not accept additional LA57-only addresses.

Host `boot::tests` use arithmetic-only descriptors for invalid addresses and
canonical boundaries; no fabricated pointer is dereferenced. Coverage includes
higher-half acceptance, canonical-hole crossing, noncanonical starts, null,
empty, alignment, end overflow, isize length, framebuffer multiplication
overflow, short pitch, and overlapping masks. No rendering test uses a fake
buffer. QEMU rendering validation is performed by the integration coordinator.

Validation commands (with shared Cargo/Rustup homes and Cargo bin on PATH):

```
cargo test --locked --offline --manifest-path rust/kernel_core/Cargo.toml --features test-r031-files-gui boot::tests
cargo check --locked --offline --manifest-path rust/kernel_core/Cargo.toml --no-default-features --target x86_64-unknown-none --features test-r031-files-gui
```

Both commands passed on this branch. The host filter ran 17 tests (including
five `boot::tests`); the bare-metal check completed with existing unrelated
compiler warnings. `git diff --check` passed.
