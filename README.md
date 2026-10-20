# IndianOS 🇮🇳

**IndianOS** is an experimental, independent desktop operating system for x86-64 PCs, designed for approachable everyday use in India. The goal is a familiar desktop workflow, Indian-language accessibility, and a native kernel—not a Linux distribution or a copy of Microsoft Windows.

> **Status: Phase 0 — architecture and planning only.** This repository does **not** yet contain a bootable kernel, installer, or production-ready operating system. Do not try to install it on a real PC.

## Initial technical decisions

- **Target:** x86-64, UEFI-first; QEMU is the only supported test platform initially.
- **Kernel:** independently developed, modular monolithic design in freestanding C11 and small amounts of x86-64 assembly.
- **Early boot:** use the open-source **Limine bootloader** and its published protocol as a temporary, well-defined handoff layer. The bootloader is *not* the kernel.
- **Graphics:** first draw pixels to the firmware-provided framebuffer; build a user-space compositor and desktop later.
- **Drivers:** bring up serial, framebuffer and timer first; then PCI, storage, keyboard and network devices.
- **Compatibility:** native IndianOS applications come first. Windows applications will **not** automatically run.

## Read the design

1. [Architecture and boot-to-desktop flow](docs/ARCHITECTURE.md)
2. [Phased development roadmap](docs/ROADMAP.md)
3. [Architecture decision record](docs/ADR-0001-foundation.md)
4. [Boot contract and error handling](docs/BOOT_CONTRACT.md)
5. [Driver bring-up and hardware strategy](docs/DRIVER_STRATEGY.md)
6. [Desktop and India-first UX plan](docs/DESKTOP.md)
7. [Milestone 001: first kernel boot](docs/MILESTONE_001.md)

## Directory plan

```text
boot/          Boot protocol adapter and future boot experiments
kernel/        Architecture, memory, interrupts, tasks, syscalls
drivers/       Device discovery and hardware drivers
userland/      Init, core libraries, services and applications
desktop/       Compositor, shell and desktop applications
tools/         Emulator, image-building and test utilities
docs/          Decisions, interfaces, roadmap, acceptance criteria
```

Directory READMEs are placeholders, not working implementations.

## First objective

In QEMU, boot an x86-64 ELF kernel using Limine, emit `IndianOS: kernel entered` on COM1 serial, validate the boot information, paint a solid-color framebuffer when available, and halt safely. See [Milestone 001](docs/MILESTONE_001.md).

## Safety and legal

Never test early kernels on a primary machine or disk. Use QEMU disk images and snapshots. Do not copy proprietary Windows binaries, artwork, trademarks or source code. Third-party firmware, bootloader and toolchain components must retain their respective licenses. The IndianOS license has **not** yet been selected.

## Scope

This is a research prototype. Consumer usability, security, commercial support, an installer, full hardware support, and application compatibility require much later milestones and substantial engineering.
