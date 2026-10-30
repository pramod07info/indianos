# Brokered graphical Terminal persistence

The qualified R031 graphical Files profile now runs a real bounded line editor
inside its PID3 Terminal. Focus Terminal with `2`, press Enter to begin editing,
and submit `write <text>` or `cat`. Tab returns to desktop shortcuts. Backspace
edits; oversized or unsupported commands cannot execute truncated input. The
note is limited to 20 printable ASCII bytes, including literal trailing spaces.

PID1 forwards input and brokers only `/shared/readme`; PID3 cannot submit a
path, PID, descriptor or user pointer to that broker. Messages use the existing
kernel-authenticated sender identities. Writes use the existing bounded durable
VFS syscall, refresh PID2 Files, and acknowledge the result. Reads return actual
persisted bytes to Terminal and its serial diagnostic. The same note is read
after reboot of the same disposable volume.

The services use syscall 35 for bounded cooperative IPC receive. This is a
scheduling-round budget, not a wall-clock timeout. The graphical Files profile
retains a finite 256-dispatch budget and 64-frame limit; other legacy profiles
retain their existing budgets. This is an experimental bounded session.

Drawing protocol operation 3 explicitly replaces an existing owned text field
with matching client rectangle. It keeps command counts bounded and repaints
retained content so shorter text erases old glyphs. It cannot create fields,
target foreign windows or alter the semantics of operations 1 and 2.

Validation commands:

```sh
make test-m1-terminal-host
make test-m1-terminal-persistence-qemu
make test-r031-files-gui-qemu
make test-r031-supervisor-qemu
```

The coordinator's new QEMU acceptance uses physical PS/2 events, framebuffer
checks, authenticated readback, two boots and unchanged neighboring disk guard
sectors. Results are recorded in the integration report, not assumed from these
host tests. Production normal startup, live GPT partition selection, general
shell commands, POSIX file permissions and an independent ring-3 compositor
remain incomplete. This change does not mark the broad M1 gates complete.
