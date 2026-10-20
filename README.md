# IndianOS 🇮🇳

**IndianOS** is an experimental, independent desktop operating system for x86-64 PCs, designed for approachable everyday use in India. The goal is a familiar desktop workflow, Indian-language accessibility, and a native kernel—not a Linux distribution or a copy of Microsoft Windows.

> **Status: Phase 1 — an initial freestanding x86-64 kernel is implemented in source and compiles as an ELF.** Limine ISO packaging and QEMU/OVMF smoke tests have been added; runtime boot verification is pending CI. This is **not** a production OS or a real-PC installer. Do not flash it to physical storage.

## Initial technical decisions

- **Target:** x86-64, UEFI-first; QEMU is the only supported test platform initially.
- **Kernel:** independently developed, modular monolithic design in freestanding C11 and small amounts of x86-64 assembly.
- **Early boot:** use the open-source **Limine bootloader** and its published protocol as a temporary, well-defined handoff layer. The bootloader is *not* the kernel.
- **Graphics:** first draw pixels to the firmware-provided framebuffer; build a user-space compositor and desktop later.
- **Drivers:** bring up serial, framebuffer and timer first; then PCI, storage, keyboard and network devices.
- **Compatibility:** long-term native, web, Linux, Windows, Android and authorized remote-Mac routes; **none runs on IndianOS today**. Our independent kernel does not automatically run other platforms' binaries.
- **AI:** planned assistant-driven app discovery and secure installation via deterministic, policy-gated package services. No AI installer exists yet.
- **Portability:** x86-64 first; ARM64 requires a separate kernel/driver port. We cannot guarantee 100% compatibility or superiority to other operating systems.

## Read the design

1. [Architecture and boot-to-desktop flow](docs/ARCHITECTURE.md)
2. [Phased development roadmap](docs/ROADMAP.md)
3. [Architecture decision record](docs/ADR-0001-foundation.md)
4. [Boot contract and error handling](docs/BOOT_CONTRACT.md)
5. [Driver bring-up and hardware strategy](docs/DRIVER_STRATEGY.md)
6. [Desktop and India-first UX plan](docs/DESKTOP.md)
7. [Milestone 001: first kernel boot](docs/MILESTONE_001.md) and [implementation guide](docs/BOOT_M001_IMPLEMENTATION.md)
8. [Cross-platform app compatibility plan](docs/COMPATIBILITY.md)
9. [AI app assistant and secure install architecture](docs/AI_APP_ASSISTANT.md)
10. [ADR: compatibility and AI](docs/ADR-0002-interoperability-ai.md)
11. [Performance and portability targets](docs/PERFORMANCE_GOALS.md)

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

The boot and kernel directories now include the early kernel code; the desktop, drivers and userspace directories remain conceptual placeholders.

## Offline app compatibility planner (development prototype)

The sample manifest contains fictional applications and planned provider routes. It is **not** a running IndianOS app store, AI model, installer, or binary-compatibility layer. On a development machine with Python 3.10+:

    python3 tools/app_resolution.py --catalog examples/app_catalog.json --device examples/device_profile.json --app "Sample Notes"
    python3 -m unittest discover -s tests -v

All execution paths are informational; no apps are downloaded or installed. The companion design and security model are documented in [AI_APP_ASSISTANT.md](docs/AI_APP_ASSISTANT.md).

## First kernel implementation (M001)

The code now contains a standalone x86-64 kernel entry, COM1 serial diagnostics, Limine protocol base revision and memory-map validation, an optional bounded tricolour framebuffer display, and safe halt behaviour. **A compiled ELF is not proof of a completed QEMU boot.**

For development on Linux/WSL2, install `clang`, `lld`, `binutils`, `make`, `curl`, `xorriso`, `qemu-system-x86`, `ovmf` and Python 3:

```sh
make toolchain-check     # verify compiler toolchain
make kernel              # compile freestanding ELF (offline)
make image               # fetch SHA256-pinned Limine binary and build ISO
make test-boot           # run UEFI QEMU smoke test against serial milestones
make run                 # interactive UEFI QEMU boot
python3 -m unittest discover -s tests -v
```

The test harness detects ordered serial markers and terminates QEMU after success; the kernel itself halts rather than exiting. Pass `OVMF_CODE=/path/to/OVMF_CODE_4M.fd` if firmware is installed elsewhere. All generated files are beneath `build/`. See [implementation details](docs/BOOT_M001_IMPLEMENTATION.md).

## Safety and legal

Never test early kernels on a primary machine or disk. Use QEMU disk images and snapshots. Do not copy proprietary Windows binaries, artwork, trademarks or source code. Third-party firmware, bootloader and toolchain components must retain their respective licenses. The IndianOS license has **not** yet been selected.

## Scope

This is a research prototype. Consumer usability, security, commercial support, an installer, full hardware support, an AI assistant, and real application compatibility require much later milestones and substantial engineering.

## GitHub contribution workflow

By project-owner preference, bounded development changes go **directly to main**. Pull requests are created only when the owner explicitly asks. Automated checks and release gates still apply.
