# R029.3 — Two independent graphical ring-3 applications

**Scope:** two separately compiled static x86-64 ELF64 applications loaded
into distinct ring-3 virtual address spaces and scheduled in a single-core
QEMU guest. The kernel grants Files (window 1) only to PID1, Terminal
(window 2) only to PID2. Each application attempts and rejects
unauthorized cross-process draws, focus and malformed user pointers.

Each process submits own-window fills and labels; the compositor
composes both command buffers in z-order into actual QEMU VGA pixels.
The GUI input broker routes authenticated focus events only to their
owners, and real emulated PS/2 mouse input is delivered to PID2 as
type-2 event. On process exit/fault, pending input and grants are
revoked, without revoking the surviving application's window.

`make test-r029-multiprocess-qemu` builds the two ELF apps and
complete kernel with the optional R029 multiprocess feature, boots QEMU,
screenshots the real VGA after each application, protects desktop
chrome, injects genuine PS/2 input and demands both PID-reap logs.

**Limit:** not a full user-space compositor; compositing and PS/2
polling remain inside privileged kernel code. No general app/service
launcher, physical hardware, or cross-platform executable support yet.
