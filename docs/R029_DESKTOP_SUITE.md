# IndianOS R029.5–R029.7 integrated development

One feature-complete QEMU acceptance unit, committed as one milestone.

## Process architecture

- PID 1: separate protected ring-3 desktop shell / presentation policy.
- PID 2: separate ELF64 Files app; holds window 1 grant only.
- PID 3: separate ELF64 Terminal app; holds window 2 grant only.
- Three independent user CR3 mappings, three privileged interrupt stacks,
  separate process-control blocks, frame tracking and exit-time cleanup.
- Readiness exchange goes through bounded kernel IPC, which authenticates
  senders using the active process rather than an asserted packet PID.
- The Rust initramfs builder safely packages all three separately built
  executables.

## Desktop features

- Per-process 64-byte draw-command copy, validation, clipping and grants.
- PID1-only desktop syscall 24: focus Files/Terminal, move Files, minimize
  Terminal, restore Terminal and toggle a visible Apps launcher.
- PID1-only frame query/present controls five explicit desktop frames.
- Actual QEMU VGA captures verify launcher, focus, movement and restore.
- Real QEMU PS/2 mouse input is delivered to the PID3 owner of Terminal.
- All three PCBs must exit and reclaim memory and event grants.

## What R029.6 does **not** complete

Although the display policy is controlled in a separate userspace process,
the final rasterization loop, physical framebuffer access and PS/2 input
polling still run in the privileged kernel. This is not a production-grade
userspace compositor. Implementing safe shared pixel buffers or dedicated
framebuffer mapping for a least-privileged renderer remains open.

Also not yet implemented: general runtime application spawning beyond the
three test ELFs, real hardware graphics acceleration, complete keyboard
scancode input routing, accessible font shaping or a default desktop boot.

```sh
make test-r029-desktop-suite-qemu
```

This explicit feature preserves earlier R029 versions and ordinary boots.
