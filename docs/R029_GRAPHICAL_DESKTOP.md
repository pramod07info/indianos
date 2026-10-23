# R029 — Real two-window graphical desktop in QEMU

## Verified graphical behavior

The R029 opt-in kernel feature renders a **real 1280 × 800 VGA framebuffer**
through the Limine 32-bit RGB mapping and QEMU's standard VGA device.
Unlike R018's headless window-state model, this paints actual pixels:

- Desktop background, top chrome, taskbar and cursor crosshair.
- Separate Files and Terminal window frames, title bars, and different
  application content, composited in Z order.
- Focus raises the selected window and changes title bar highlighting.
- An actual QEMU emulated PS/2 auxiliary mouse packet updates cursor
  position and left-click window focus; titlebar dragging is supported.
- COM1 keyboard controls focus, move, minimize, restore and close.
- All pixel writes are clipped to validated framebuffer geometry, and
  the original boot-mode video/serial paths remain unchanged.

The graphics path is gated behind `test-r029-desktop`. It does not run
in normal IndianOS boot, does not touch physical graphics hardware, and
does not allocate or share user framebuffer mappings.

## Interactive keyboard controls

| Key | R029 compositor operation |
|---|---|
| `1` | Focus Files |
| `2` | Focus Terminal |
| `h` | Move focused window 36 pixels right, clamped to screen |
| `m` | Minimize focused window |
| `r` | Restore the active window |
| `x` | Close the selected independent window |
| `q` | End the graphical demonstration |

The PS/2 mouse uses bounded three-byte packet decoding and polling with
IRQ masking. The separate IRQ1 keyboard test remains unaffected.

## Independent QEMU visual acceptance

Reproduce with a single command on a machine with Rust x86-64 freestanding
target, Clang/LLD, QEMU, OVMF and xorriso:

```sh
make test-r029-desktop-qemu
```

The test script `tools/test_r029_desktop_qemu.py` boots an opt-in ISO on
QEMU's actual standard VGA display, captures screenshots via its HMP
`screendump` monitor command and parses raw P6 RGB data. It requires
two distinct visible window surfaces and at least six independent colors.
It drives QEMU's physical PS/2 emulation via `mouse_move`/`mouse_button`,
checks a real Files-window focus log and changed screenshot pixels,
then sends seven distinct serial keyboard events. Each event must
alter the framebuffer snapshot; close must leave one window. The test
quits through the UI and verifies the kernel success marker.

[Live R029 passing QEMU run](https://github.com/pramod07info/indianos/actions/runs/39941297124)
confirmed a 1280×800 VGA framebuffer, an actual PS/2 mouse selection and
all keyboard focus, move, minimize, restore and close changes.

**Regression gates:** host C ABI compilation/linkage, GUI clipping and
window model unit tests, PS/2 packet decoder tests, freestanding
`no_std` build, Rust ownership audit, and the real VGA test. All
run in R029 verification CI and the VGA test is required by full
`main` kernel CI.

## Strict architecture limitation

This is a **kernel-mode test compositor** rather than a production
user-space graphical service. File-manager/terminal interiors are
static independent UI panels; there is no user-supplied application
surface protocol or user-mode permissions for draw commands.

A **true user-space compositor**, persistent desktop session, mouse IRQ
service, GPU acceleration, Unicode text shaping, app integration,
resizable windows, drag/drop, overlapping child surfaces and accessibility
remain unfinished. R029's original user-space isolation ambition is
not satisfied by this real-pixel demonstration alone. No Windows/Linux/
macOS application compatibility is implied.

## Subsequent R029 ring-3 window command bridge

The first real QEMU graphical demo rendered two windows in ring 0. An
additional opt-in `test-r029-userspace` mode now launches a separately
mapped ELF64 client in **ring 3**, rendering two distinct client-area
overlays through kernel-authorized `SYS_GUI_DRAW=18`, `SYS_GUI_FOCUS=19`
and `SYS_GUI_STATUS=20`. This is a strict first step toward a
userspace window server:

- Only the actual running PCB PID1 with a verified executable
  syscall origin can submit graphics commands. Sender PID is not
  supplied in the message.
- Commands use a fixed 64-byte versioned wire record; the kernel
  verifies a complete mapped user buffer then copies it before
  parsing. No physical framebuffer pointer or kernel address
  is accepted.
- Window identifiers are limited to 1 and 2, the Files and Terminal
  demo. At most eight paint operations per window can be replayed.
- Colors, text bytes, rectangle geometry and client-area clipping
  are all validated before rendering. The compositor respects
  z-order and preserves desktop chrome.
- Rejected malformed user buffers cannot mutate the prior scene.
- The original ring-3 process cleanup returns page ownership
  without leaks.

[Passing ring-3 QEMU visual isolation verification](https://github.com/pramod07info/indianos/actions/runs/39949896007)
captures the actual VGA desktop before any ring-3 drawing, after
each user-mode window update and after a user-mode focus change.
An independent host assertion checks pixel changes and verifies
protected top and bottom bars remain unchanged.

**Still missing:** a separate compositor running in userspace,
independent per-app authorization, a shared-memory surface
allocator, keyboard/mouse event delivery to apps, and a real
Files/Terminal application protocol. The framebuffer and the
authoritative window manager remain trusted kernel services.
