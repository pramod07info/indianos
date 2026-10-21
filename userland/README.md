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
