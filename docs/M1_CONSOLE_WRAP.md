# M1 framebuffer console delayed wrap

The real software framebuffer console now holds the cursor at the last column
with a pending-wrap flag after drawing a full line. The next supported printable
ASCII byte consumes that flag and advances before painting. Newline advances
exactly once; carriage return cancels pending wrap and returns to column zero.
Backspace with pending wrap erases the just-painted last cell and leaves the
cursor there, allowing replacement without advancing or scrolling. Backspace at
column zero remains bounded and does not cross into the preceding row. Ignored
control bytes leave pending wrap untouched.

This prevents premature scrolling when the bottom-right cell is printed. Cursor
coordinates remain inside the owned cell rectangle, including a single-cell
screen. Initialization still requires the existing minimum display size. Glyphs,
ASCII support, volatile pixel writes, scroll limits, and the synchronous BSP
caller contract are unchanged; this does not add IRQ or multi-core rendering.

The console tests allocate aligned u32 framebuffer memory and invoke production
put/paint/advance through ConsoleFramebuffer. They check actual foreground and
background pixels for last-column newline, backspace, carriage return, deferred
bottom-right printable scroll, bottom-right newline scrolling once, and 1x1 and
1x2 cursor states. Guards, pitch padding, fractional-cell right/bottom strips,
and extra scanlines retain sentinel pixels after rendering and scrolling.

Validation commands (locked, offline; shared CARGO_HOME and RUSTUP_HOME):

```
cargo test --locked --offline --manifest-path rust/kernel_core/Cargo.toml --features test-r031-files-gui console::tests
cargo check --locked --offline --manifest-path rust/kernel_core/Cargo.toml --no-default-features --target x86_64-unknown-none --features test-r031-files-gui
```

QEMU integration remains the coordinator's responsibility.

Results: all nine focused console tests passed (seven real framebuffer fixtures
plus two existing glyph tests); the no_std x86_64-unknown-none check passed.
Both commands emitted existing repository warnings unrelated to console wrapping.
