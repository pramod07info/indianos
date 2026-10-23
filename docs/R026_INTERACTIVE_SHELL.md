# R026 — Persistent interactive QEMU init with lifecycle-controlled worker

## Verified behavior

R026 extends the original R013/R014 shell so it can **launch and reap a
worker while the shell itself remains alive**. The shell uses the serial
console available through QEMU/OVMF. The kernel stages two independent
ring-3 ELF processes with separate stacks and address spaces. The worker
begins in a nonrunnable state and is enabled only after PID1 successfully
requests the allowlisted `/bin/worker` executable.

### Interactive commands

| Command | Behavior |
|---|---|
| `help` | List allowed operations |
| `ps` | Show PID1 and whether worker has started or been reaped |
| `run /bin/worker` | Launch exactly one prepared worker and yield; shell resumes after child is reaped |
| `wait /bin/worker` | Confirm worker lifecycle completion |
| `history` | Recall the last command using existing shell input support |
| `version` | Report shell version |
| `exit` | Exit shell explicitly and return kernel to completed test state |

Unauthorized worker syscalls are rejected. The worker cannot spawn its
own subprocess or inspect init-only process state. A second worker
launch request fails. The shell's `ps` and `wait` calls accept only the
allowlisted child PID. The user buffer and syscall metadata protections
are inherited from the existing ring-3 syscall implementation.

## Reproduction and acceptance

```sh
# Requires QEMU x86_64, OVMF, xorriso, Clang/LLD and Rust cross-target.
make test-r026-interactive-qemu
```

The serial smoke runner sends seven commands only after observing each
fresh shell prompt. It requires all shell/worker serial markers in order,
one worker launch grant, an observed worker reap before shell exit,
one reap for each PID, and a bounded overall test duration.

An isolated GitHub Actions verification job passed:
https://github.com/pramod07info/indianos/actions/runs/39924988234

## Explicit boundaries

This is an **opt-in QEMU-only** milestone; normal boot does not turn on
the interactive shell. It is not a general-purpose init/service manager:
the worker executable is known at build time, only one launch is allowed,
and there is no dynamic program registry, shell pipelines, POSIX terminal
semantics, multiuser session manager, background daemon orchestration,
or general-purpose process tree. Never mistake this for production
workstation readiness.
