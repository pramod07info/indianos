# M1 retained-client compositor visibility

The live R029 `UserSurfaceService::redraw` previously skipped hidden/minimized
window chrome inside `draw_window`, then replayed that window's client commands
unconditionally. A minimized foreground app could therefore paint over a visible
peer's content and titlebar without displaying its own chrome.

The existing back-to-front raster loop now skips a non-visible or minimized
window before drawing either chrome or retained client commands. Closed windows
are absent from that same desktop iterator. The command backing arrays remain
independent and intact; an explicit authorized server focus restores a minimized
window and its retained content. The low-level window painter also checks
visibility before selecting either the standard or Orbit renderer.

`redraw` obtains the validated boot framebuffer and calls `paint`. A `Surface`
carries its RGB encoder: boot uses `boot::desktop_rgb`, while host pixel tests use
an ordinary memory buffer with explicit RGB packing. The raster algorithm,
clipping and z-order loop are identical. `present_frame` calls `present_with`,
which retains the production PID authorization, command-count/front validation,
focus and successful-frame accounting, then invokes the production painter.
This small destination seam lets host tests exercise real writes without firmware
pointers or PS/2 port access.

The memory framebuffer test submits parsed fill/text commands through the real
owner-gated service for PID2 Files and PID3 Terminal. Their windows overlap so
Terminal's client rectangle covers Files' titlebar. It verifies:

- Foreground client pixels are present before minimization.
- Minimization removes Terminal pixels from both peer chrome and client content.
- Foreign submit/present callers and invalid front IDs leave pixels, backing,
  hidden state and frame counters unchanged.
- Explicit server restoration returns Terminal's retained pixels.
- Raising Files preserves its chrome and content over Terminal's backing.
- Closing Terminal excludes its retained pixels and rejects further submissions.
- Framebuffer row padding and trailing guard words remain untouched.

Validation commands (offline, locked manifest):

```sh
CARGO_HOME=/workspace/.cargo RUSTUP_HOME=/workspace/.rustup PATH=/workspace/.cargo/bin:$PATH cargo test --locked --offline --manifest-path rust/kernel_core/Cargo.toml --features test-r029-desktop-suite gui::tests
CARGO_HOME=/workspace/.cargo RUSTUP_HOME=/workspace/.rustup PATH=/workspace/.cargo/bin:$PATH cargo build --locked --offline --manifest-path rust/kernel_core/Cargo.toml --no-default-features --target x86_64-unknown-none --features test-r029-desktop-suite
```

The focused desktop-suite passes five tests. The retained-client regression
also passes with `test-r031-files-gui,test-r031-supervisor`: its fixture submits
the required five Files startup commands under that feature and retains the
resulting command counts for the ownership assertions. A regression probe that removed only the
new replay visibility guard failed the pixel test at the exposed Files titlebar;
the fixed source was restored afterward. Existing unrelated compiler warnings
remain. QEMU integration is run by the batch coordinator using the existing
R029 desktop suite; no QEMU result is claimed here.

This fixes a concrete bug in the opt-in kernel raster backend. It does not
complete a full userspace compositor or general display-driver/GPU ABI.
