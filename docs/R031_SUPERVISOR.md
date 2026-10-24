# R031.2 — Strictly bounded crash recovery and desktop permissions

Security design: Only the kernel scheduler is allowed to restart one
**allowlisted Terminal service** after the x86-64 `#UD` fault. This is an
opt-in QEMU-only test feature, `test-r031-supervisor`, disabled for normal
boots and all older R029/R030 tests.

## Fault and restart sequence

1. Three separate ring-3 ELFs are loaded with separate CR3 address spaces.
2. PID 2 (Files) registers a message. PID 3 (Terminal) draws a bounded
   command and executes the actual `ud2` instruction.
3. Kernel validates the fault vector and PID. It unregisters PID 3's IPC
   mailbox, revokes window ownership and pending mouse events, destroys
   the old address-space mappings and user frames, and reaps PID 3.
4. The kernel appends PID 4 using the original **trusted boot-staged Terminal
   ELF**, a fresh process-control block, fourth trap stack and new CR3.
5. A strict one-restart budget permits GUI window-2 access to PID 4 only
   after it is prepared, with a separately registered IPC identity.
6. PID 1 receives the new PID via kernel-only status syscall 26, accepts
   the replacement readiness notification and presents the desktop.
   Userspace cannot claim to be PID 4 by supplying an arbitrary integer.
7. Terminal PID 4 and Files PID 2 must continue to be denied desktop-frame
   presentation and privileged session manager operations. PID 4 can
   submit only window-2 drawing commands and receive owned mouse events.
8. The session remains interactive (launcher, focus, move, minimize, restore).
   `q` shuts down the two healthy apps; all four process records are reaped
   and page allocations are restored to the pre-session count.

The recovery policy is implemented in `service_supervisor.rs` and tested for
invalid identities, vector mismatches, stale generations and retry exhaustion.

## Run

```sh
make test-r031-supervisor-qemu
```

The script captures actual VGA screenshots and sends genuine PS/2 mouse
packets to an isolated x86-64 QEMU VM. It never connects to the host's disks.

## Important limitations

Only one explicitly allowlisted Terminal **crash** is restartable; a second
fault or an unexpected vector fails the test instead of retrying. This
does **not** implement general production supervisor policy, watchdog-based
hang recovery, automatic persistence of unsaved Terminal state, dynamic
service registration, multi-core dispatch, OS user authentication, secure
boot, kernel-independent rasterization or hardened arbitrary application
packages. Those remain separate engineering milestones.
