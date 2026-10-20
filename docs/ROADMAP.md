# IndianOS development roadmap

> Planning estimates assume an experienced engineer and a small open-source team; they are not delivery commitments. **An everyday Windows alternative is a multi-year, multi-person effort.**

| Phase | Suggested horizon | Objective | Exit condition |
| --- | --- | --- | --- |
| 0. Design | 1–2 weeks | Repo structure, initial contracts, threat model | Reviewed ADRs, milestone checklist |
| 1. First boot | 2–6 weeks | ELF kernel boots through Limine in QEMU | Serial `kernel entered`, safe halt, optional framebuffer |
| 2. Kernel foundations | 2–5 months | GDT/IDT, exceptions, PMM, VMM, timers, heap | Memory/exception smoke tests in emulator |
| 3. Tasks and user mode | 3–6 months | Ring-3 process, context switch, syscalls, IPC | Isolated hello-world user process |
| 4. Storage and devices | 4–9 months | ACPI/PCI, virtio-block, simple VFS, initramfs, input | Read files and accept keyboard in QEMU |
| 5. Services and networking | 4–9 months | Init, service lifecycle, virtio-net, TCP/IP subset | Ping / basic network-client demonstration |
| 6. Graphical desktop alpha | 6–12 months | Framebuffer compositor, text, windows, mouse, shell | Launch two independently rendered windows |
| 7. Developer alpha | 6–12 months | Toolchain, user libraries, APIs, applications, update design | Repeatable app builds + signed update prototype |
| 8. Hardware and security beta | 12–24+ months | Selected PC models, input/storage/network drivers, isolation hardening | Automated boot tests and physical-hardware qualification |
| 9. Public consumer release | Years | Installers, recovery, localization, broad compatibility | Security, accessibility, UX and support benchmarks |

The phases overlap only after team capacity and automated tests permit it. Numbers should be revised after Phase 1 measurement; **do not sum these estimates into a promised release date**.

## Priority backlog (dependency order)

1. P0: reproducible x86-64 cross-build; pinned Limine+protocol; QEMU+OVMF.
2. P0: serial logger, panic handler, kernel entry; fail on missing required boot responses.
3. P0: validate memory map and framebuffer; clean halt; boot smoke tests.
4. P1: CPU exceptions, GDT/IDT, page frame allocator, virtual memory.
5. P1: timer, scheduler and isolated user-space task.
6. P1: syscall ABI, initramfs, VFS, init.
7. P2: keyboard, mouse, virtio-blk, virtio-net, PCI/ACPI.
8. P2: userspace window server, compositor, shell and basic apps.
9. P3: real-hardware matrix, updates, secure boot/recovery, application ecosystem.

## Governance and risk gates

- Every phase requires a written exit test and regression tests in QEMU.
- No prototype binary should be advertised as installable or secure.
- Isolate drivers and firmware assumptions; plan crash reports and debug logging early.
- Treat licensing and name/trademark clearance as prerequisites for distribution.
- Windows application compatibility is *not* included in alpha; document a native app ABI first.

## What "done" means

- Phase 1: `make run` (once implemented) yields an observable kernel log in QEMU.
- Phase 3: a ring-3 process cannot read/write kernel memory.
- Phase 6: graphical windows are managed by a user process, not a kernel task.
- Beta: qualified hardware, security review, installer/recovery and update rollback.

## Cross-platform and AI app delivery tracks (future)

These tracks depend on a working isolated userspace; they are not replacements for the kernel milestones above:

- **Native/web:** real package management, signed app manifests, browser/PWA runtime, app-store UX and rollback.
- **Linux apps:** choose a bounded syscall/ABI target, ELF loader, POSIX compatibility test suite, sandbox policy and verified sample apps.
- **Windows apps:** evaluate a portable Windows user-space API layer and separately licensed Windows VM sessions; verify each app and driver requirement.
- **Android apps:** validate kernel/graphics requirements for an Android runtime or isolated guest.
- **macOS-only apps:** prioritize native ports, web versions and approved remote Mac execution. Do not promise generic non-Apple hardware virtualization.
- **AI app assistant:** natural-language request -> trusted catalog -> deterministic compatibility decision -> user approval -> signed transactional package service -> verification/rollback.
- **Hardware portability:** an ARM64 port is a separate platform workstream, requiring its own CPU, MMU, interrupt and device support.

Tracking metrics: number of **verified** supported applications, compatibility regressions, cold-boot time, app launch time, crash rates, idle resource use and install recovery. The 100% universal compatibility aspiration cannot be promised or treated as an acceptance criterion.

See [COMPATIBILITY.md](COMPATIBILITY.md), [AI_APP_ASSISTANT.md](AI_APP_ASSISTANT.md) and [PERFORMANCE_GOALS.md](PERFORMANCE_GOALS.md).
