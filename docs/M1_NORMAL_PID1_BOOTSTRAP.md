# Normal boot: native PID1 bootstrap console

Ordinary `make image RUST_CORE=1` now starts the validated `/bin/init` ELF as
PID1 in ring3, using the existing guarded user mappings, TSS entry, syscall
validation and PCB ownership. Previously normal boot staged/released the ELF
and halted, leaving its default executable as an infinite pause fixture.

The automatically selected production `normal-init` profile starts a bounded
**serial bootstrap console**, independent of the experimental desktop flags.
PID1 checks ABI version and its authenticated process ID before reporting ready;
`status` rechecks both. `help`, `status` and `halt` are the only commands.
Filesystem and desktop are explicitly reported offline. There is no disk
mount, format, service launch, raw memory command or automatic retry.

Only PID1's running PCB can read bootstrap UART input. The writable byte is
validated before reading the UART, then copied through checked user mappings.
This production route preserves raw control bytes; UART line-error transfers
become a poison byte, preventing `ha<NUL>lt` from becoming `halt` through input
filtering. The 32-byte userland parser discards an invalid/overflowed command
through its newline, supports bounded backspace and treats CRLF as one ending.
It does not parse ANSI escapes. This initial console is serial-only, polling
on the BSP with user interrupts masked; it is not an IRQ input service.

`halt` exits through INT 0x80. The kernel releases the user tables, private CR3
clone and physical frames, reaps the PCB, then enters existing read-only kernel
recovery. It stops PID1, **not ACPI poweroff**. A process failure enters recovery
instead of restarting over uncertain state. Existing experimental profiles keep
their prior paths and default negative boot tests retain recovery behavior.

## Reproducible acceptance

- `make test-normal-init-host`: four parser regressions, including every
  unsupported byte, poisoned suffixes, exact capacity, backspace and CRLF.
- Kernel host tests with `--features normal-init`: UART transfer preservation
  and error poisoning plus existing kernel regressions.
- `make test-normal-init-qemu`: real normal-profile OVMF boot, native PID1
  startup, input-dependent help/status, embedded NUL and overflow rejection,
  INT 0x80 exit, PMM frame restoration, PCB reaping and read-only recovery.
  A writable disposable sentinel IDE disk must remain byte-for-byte unchanged.
- `python3 -m unittest discover -s tests -p test_normal_init.py`: acceptance
  trace negative controls; these validate the harness, not guest execution.
- Existing `make test-boot-recovery-qemu` and normal smoke remain regressions.

No full M1 gate is newly accepted from this slice. Normal graphical startup,
production PID1 supervision, independent compositor, selected-partition storage,
multicore scheduling and physical UEFI qualification remain outstanding.
Coordinator `make test-normal-init-qemu test-boot-recovery-qemu` passed the real
normal PID1 scenario and all four damaged-init recovery scenarios with unchanged
sentinel storage. The Make target forwards the selected QEMU/OVMF paths rather
than assuming a firmware filename. Exact-head GitHub checks remain merge gates.
