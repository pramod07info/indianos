# M1 PS/2 Set-1 state recovery

CapsLock now tracks its physical down state. Repeated make bytes while held
leave the committed lock setting unchanged; a break permits the next press to
toggle it. The earlier balanced-case fixture now includes a real CapsLock break
between its two presses, preserving all existing assertions.

Both keyboard polling and IRQ1 pass consumed primary bytes through the same
status-aware decoder entry. A parity/timeout-marked primary transfer is consumed
without generating a character and clears transient Shift, Ctrl, Alt, Caps-down,
E0 and Pause state. The committed CapsLock choice survives. Empty status and AUX
status do not alter keyboard state. Polling and IRQ1 leave AUX data in the
controller for the mouse owner; neither path steals a mouse packet byte.

Recovery begins with the next valid transfer as a fresh Set-1 byte. This is a
bounded best-effort resynchronization, not reconstruction of the lost event:
held modifiers must be pressed again, and the prior physical Caps-down state is
unknown. The discarded transfer itself never invents text or increments the
accepted physical-input counter. Existing unmodified shortcut filtering and
counting at accepted-character delivery remain unchanged.

Regression coverage includes repeated Caps makes across two physical presses,
left/right Shift and left/right Ctrl/Alt recovery for parity and timeout errors,
E0/Pause prefix reset, committed lock preservation, AUX/empty status isolation,
and the existing genuine accepted-scan counter and shortcut assertions.

Validation (offline, locked, `CARGO_HOME=/workspace/.cargo`,
`RUSTUP_HOME=/workspace/.rustup`):

- `cargo test --manifest-path rust/kernel_core/Cargo.toml --locked --offline --features test-r031-files-gui input::`: 31 passed, 0 failed (includes matching `m1_input::` tests).
- `cargo check --manifest-path rust/kernel_core/Cargo.toml --locked --offline --no-default-features --features test-r031-files-gui --target x86_64-unknown-none`: passed (19.74s). The initial invocation without `--no-default-features` retained the host `std` default and was stopped after unavailable-`std` errors; the corrected command checks the bare-metal configuration.

Real QEMU integration belongs to the coordinator's combined batch validation;
this isolated change does not claim a QEMU run.
