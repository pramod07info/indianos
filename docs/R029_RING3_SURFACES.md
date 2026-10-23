# R029 — Ring-3 validated graphical surface commands

## What is implemented

An opt-in `test-r029-userspace` QEMU mode launches a real ELF64
`/bin/init` in ring 3. The program controls the *content* of two
independently composited Files and Terminal windows via a narrow graphics
syscall interface. Its user pages are separated from the privileged
framebuffer, which remains mapped only in the trusted kernel.

This milestone preserves the existing R029 QEMU mouse/keyboard screenshot
test, the Rust C-ABI host linkage regression, and the normal boot path.

## Syscall contract

The process enters software interrupt `int 0x80`:

| Number | Name | Argument 0 | Argument 1 | Behavior |
|---|---|---|---|---|
| 18 | `GUI_DRAW` | user pointer to 64-byte wire command | exactly 64 | append validated fill/text operation to allowed client region; 0 or `u64::MAX` |
| 19 | `GUI_FOCUS` | valid window ID 1 or 2 | 0 | reorder windows and replay backed client surfaces; 0 or `u64::MAX` |
| 20 | `GUI_STATUS` | 0 | 0 | read window command counts, focus events and currently focused window |

Wire message little-endian:

| Byte range | Value |
|---|---|
| 0–3 | ASCII `R29U` |
| 4–7 | format version `1` |
| 8–11 | existing window ID `1` or `2` |
| 12–15 | opcode `1` (solid fill), `2` (ASCII text) |
| 16–23 | unsigned 16-bit x, y, width, height relative to the window's client area |
| 24–27 | 24-bit RGB `0x00RRGGBB` |
| 28–31 | required zero |
| 32–63 | up to 24 printable ASCII bytes, NUL terminated for text, all-zero for fills |

A render operation is rejected before mutation if it is outside the
window's client interior, too large, malformed or would exceed
eight retained commands per window. Text must fit its bounded
monospaced glyph width and height. The kernel copies the **entire
verified userspace descriptor** into private bytes, validates its
actual process identity and executable syscall origin and redraws
Windows with the correct Z order.

There is no raw framebuffer-memory syscall. Directly supplying an
unmapped, supervisor-only or otherwise forged address fails.
All user-owned buffers are checked through the existing per-process
page table ownership mechanism.

## QEMU acceptance

```sh
make test-r029-user-surfaces-qemu
```

`tools/test_r029_user_surfaces_qemu.py` boots QEMU/OVMF with a
real 1280×800 standard VGA display. The host captures actual
HMP `screendump` frames for the initial desktop and three stages,
advancing the client with COM1 input between stages. It checks:

- The Files client contents changed **229** sampled pixels.
- The Terminal client contents changed **279** sampled pixels.
- Focus reordering changed **1,532** sampled pixels.
- The desktop title bar and taskbar kept their exact RGB pixels.
- Invalid user-memory pointers, illegal geometry and forged
  window IDs are refused without changing command counts.
- PID1 exits and is reaped, and its physical memory allocation
  count is fully restored.

[Passing R029 ring-3 QEMU visual test](https://github.com/pramod07info/indianos/actions/runs/39949896007)
also includes **117 passing Rust tests**, a host ABI linkage regression,
freestanding compilation and source auditing.

## Explicit limitations

This is not yet a full independent userspace compositor. The kernel
still owns the framebuffer, window placement, input routing and
the compositor. Only the graphics command *client* executes in
user mode and controls demonstration content. Per-app isolation,
multiple concurrent GUI processes, event queues, resize negotiation,
window-server permissions, production video hardware and a
persistent graphical desktop session remain future work.

All of these capabilities are **disabled in ordinary boot**.
