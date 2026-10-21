# IndianOS Rust userland (R013)

R013 introduces an **opt-in scripted shell** implemented as a freestanding
Rust ELF64 `/bin/init` running at CPL3. The command parser is allocation-free
and recognizes exactly `help`, `ps`, `run /bin/worker`, and `exit`.
It executes a fixed boot script, **not** keyboard input or arbitrary user
commands. `ps` currently reports only the running PID1 using a new validated
`GETPID` syscall. `run /bin/worker` sends a restricted numeric spawn
request through INT 0x80; the kernel launches the known worker after PID1
exits, using a fresh process ID, address space and independent reaping.

The old demonstration programs, cooperative scheduling, faults and inert
normal-boot fixture remain unchanged unless `R013_SHELL_TEST=1`.

Try `make test-rust-shell-host` and `make test-rust-shell-qemu`
(QEMU/OVMF and Rust target required). Interactive terminal/keyboard input,
arbitrary path lookup from userspace, asynchronous process spawning,
pipes, files and a complete shell are **not implemented yet**.

## R014 real input (opt-in)

Build with `R014_INTERACTIVE_TEST=1` to replace R013's fixed command
script with a live keyboard/serial input loop. In QEMU's `-serial stdio`
console, enter `help`, `ps`, `run /bin/worker`, and `exit`.
The terminal supports ASCII, Backspace and Enter. The optional legacy
PS/2 scan-code input is polled, not interrupt-driven, and input/output
is currently displayed through the serial console (not as framebuffer
terminal text). `run` queues a single worker for **after** PID1 exits.
Use `make test-rust-interactive-qemu` for an automated console replay.
