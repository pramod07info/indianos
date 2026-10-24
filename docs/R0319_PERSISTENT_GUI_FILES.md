# R031.9 — Persistent graphical Files integration

# R031.9 Files is a real ring-3 client of the persistent /shared disk VFS,
# not an on-host mock and not raw disk access. Only PID1 can write; PID2's
# listing and open operations are restricted to /shared.

## Included functionality

- Opt-in `R031_FILES_GUI_TEST=1` QEMU kernel enables the existing independent
  PID1 session manager, PID2 Files and PID3 Terminal plus durable R027 storage.
- PID1 mounts an *isolated disposable 128-sector QEMU disk* and provisions
  `/shared/readme` on first boot. Later boots detect the existing file and
  preserve its content rather than formatting it.
- PID2 lists real durable `/shared` contents with new **read-only syscall 34**.
  Only PID2 may enumerate precisely `/shared`, and output is limited to
  72 verified user bytes. Attempts to list private paths are denied.
- PID2 opens and reads `/shared/readme` via process-owned file handle syscall
  33, renders the actual filename and file content in its GUI window and
  closes its handle. No access to the framebuffer or another app's window.
- Pressing **f** in the desktop has PID1 atomically overwrite the actual file,
  then notify Files over authenticated IPC; PID2 reads the new bytes, updates
  the graphical preview and acknowledges the refresh. One update per desktop
  session is bounded by the eight-command per-window GUI budget.
- Two separate QEMU boots of the same scratch disk test that the graphical
  Files window displays the updated content after reboot. Mouse interactions,
  screenshot pixels, reaping, and guard sectors are validated.

## Build and test

```bash
make test-r031-files-gui-qemu OVMF_CODE=/usr/share/OVMF/OVMF_CODE_4M.fd
```

Still experimental: this is **not** a general file explorer or POSIX desktop.
One shared directory and file is handled by this test path; no arbitrary
navigation, multiple refreshes, drag-and-drop, multi-user ACLs, installer or
physical hardware support. Other desktop and VFS QEMU tests remain separate.
