# R029.2: physical PS/2 mouse events to ring 3

Experimental real-emulator integration, disabled in normal boots. Uses
the existing 1280×800 framebuffer, authenticated window broker, 16-byte
GUI_EVENT_POP syscall, and PS/2 8042 auxiliary mouse driver. The kernel
resolves window ownership and emits type-2 pointer-focus events after real
emulated input packets. The ring-3 ELF verifies the event's sequence,
window and type, and QEMU independently checks changed framebuffer
pixels with screen dumps.

The ring-3 process exit/fault path now revokes its window grants and
pending input before PCB reaping. Kernel core unit tests validate
cross-process denial, physical event type and process revocation.

Acceptance command: `make test-r029-physical-qemu` with QEMU,
OVMF, Limine, Rust bare-metal target, and Clang/LLD installed.

**Boundaries:** one GUI PID still controls two demo windows. Framebuffer,
compositor and physical-input hardware polling remain privileged.
This is not a complete independent userspace window server, general
pointer IRQ system, hardware-installer or third-party app platform.
