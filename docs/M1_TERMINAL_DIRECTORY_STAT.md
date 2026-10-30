# Bounded live Terminal directory and size commands

The experimental three-process Files desktop now accepts `ls` and `stat`
in its real ring-3 Terminal editor. `ls` requests the actual `/shared`
directory through PID1's existing filesystem syscall; `stat` reads the
actual `/shared/readme` and reports its byte length. No client-selected
path, PID, pointer, descriptor or metadata is trusted. IPC authenticates
PID3 requests and PID1 responses through the existing bounded exchange.

Directory responses must be complete newline-terminated printable records
that fit the 20-byte text field. Empty directories display EMPTY DIRECTORY;
truncated, oversized, control-containing or failed responses display IO ERROR.
The stat operation is deliberately a fixed-note byte-count operation, not
a general POSIX stat interface. Reads do not consume the write budget.
Existing write/cat behavior and the persistent format remain unchanged.

Six parser tests and eight Python harness controls pass locally. The real
QEMU harness additionally types ls/stat through physical PS/2, verifies fresh
exact guest responses and changed framebuffer pixels on both same-disk
boots, alongside the existing write/cat/persistence/cleanup assertions.
Worker Docker execution stalled before starting. The coordinator subsequently
ran the actual two-boot QEMU acceptance successfully, including physical ls/stat,
durable write/readback, framebuffer changes and all three PCB cleanups. This
does not qualify normal desktop boot, full shell/navigation, an independent
compositor or physical hardware.
