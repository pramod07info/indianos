# IndianOS R031.1 — Automatic three-process desktop session

This is a **single-core x86-64 QEMU-only experimental** automatic GUI startup
milestone, distinct from the scripted R029.5–7 desktop verification.

## Process startup contract

- The kernel boots and creates three isolated ring-3 process/address spaces.
- PID1 (`/bin/init`) is the desktop session manager and launcher policy owner.
- PID2 (`/bin/worker`) paints the Files application with window 1 capability.
- PID3 (`/bin/terminal`) paints Terminal with window 2 capability.
- Both applications register readiness through authenticated bounded IPC,
  and the session manager presents the first frame **without any host
  keyboard/input prompt**.
- Only PID1 may open/close the launcher, focus/move/minimize/restore windows,
  pump genuine hardware mouse input and present the complete display.
- PID2/PID3 cannot use the session-manager syscall 25 or other display
  privilege controls.
- `a` toggles Apps; `1` and `2` focus Files/Terminal; `h` moves Files;
  `m` minimizes Terminal; `r` restores it; `q` shuts down the session.
  These commands currently come from the QEMU COM1 serial console.
- On exit, PID1 uses IPC to request child shutdown. The scheduler validates
  all three process exits, owned-event cleanup and exact PMM reclamation.

## Reproduce

```sh
make test-r031-boot-desktop-qemu
```

QEMU acceptance checks the real VGA framebuffer after automatic startup,
eight user actions, actual PS/2 mouse packets, and orderly 3-process cleanup.

## Security and support boundaries

This is **not yet the default production kernel boot configuration**. The
feature `test-r031-boot-desktop` enables the new session to avoid changing
existing R029/R030 tests. The desktop rasterizer still runs in ring 0;
non-graphical hardware, keyboard IRQ routing and the launcher are not a
general-purpose app manager. Only two built-in apps are shown. No filesystem
mounts, dynamic app injection or network access are performed by this mode.

R031.2 permissions, service supervision/restarts, watchdogs, stable
system-service APIs and actual userspace compositor memory mapping remain
future milestones.
