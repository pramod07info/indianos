# R029.4 — Isolated ring-3 frame server with deferred presentation

**One milestone, one commit.** Two freestanding Rust ELF64 programs run in
distinct memory mappings under the established two-PID scheduler.

- PID1 is an isolated ring-3 frame server which grants **itself no access**
  to PID2's drawing syscall. It owns window 1 and controls when frames
  containing both surface buffers are displayed.
- PID2 is a separate ring-3 Terminal application that owns only window 2.
  It can enqueue validated draw commands but cannot query or invoke
  compositor frame-presentation syscalls 22 and 23.
- New `SYS_GUI_FRAME_QUERY=22` returns packed per-surface buffered command
  counts and frames already displayed to authenticated compositor PID1.
- New `SYS_GUI_FRAME_PRESENT=23` takes the desired front window (1 or 2)
  and requests one clipped, kernel-backed render pass. Only PID1 is
  authorized; malformed requests and a third frame fail closed.
- Submitting draw commands and physical PS/2 mouse focus events does
  **not** automatically repaint the VGA framebuffer in this mode.
  Presentation occurs only when the ring-3 frame server requests it.
- Actual QEMU screenshots prove both apps' commands were not displayed
  until the first server request, PS/2 input did not force a frame,
  and the second server request painted the focused Terminal window.
- Both process exit paths revoke owned windows and events. Cross-PID
  access, pointer spoofing, system chrome and unbounded present attempts
  are denied; existing R029.1-3 modes remain unchanged.

```sh
make test-r029-frame-server-qemu
cargo test --offline --manifest-path rust/kernel_core/Cargo.toml --features test-r029-frame-server
```

**Still pending:** The final raster loop remains in ring 0. This is a
functional user-controlled *frame policy* and presentation boundary, not
a full ring-3 framebuffer compositor. Future work must move window
rasterization/text into an independently memory-isolated userspace
compositor without granting raw framebuffer or PS/2 ports to untrusted
applications. The milestone is QEMU-only, not a supported desktop OS.
