# IndianOS system architecture (draft 0.1)

## Guiding principles

1. **Independent kernel:** all kernel primitives, syscall ABI and driver interfaces belong to IndianOS. We may rely on open firmware, development tools and a bootloader.
2. **Bootable before beautiful:** establish observable, repeatable QEMU boot milestones before UI.
3. **Separate privilege levels:** user programs, compositor and device services cannot be treated as kernel code.
4. **Narrow hardware target:** x86-64 UEFI/QEMU first; real laptops later, after robustness testing.
5. **Reproducible and safe:** pin third-party versions, automated image builds, no host-disk writes.

## Logical architecture

```text
  +---------------------------------------------------------+
  | IndianOS Apps: Files / Settings / Browser / Store / AI  |
  +---------------------------------------------------------+
  | Desktop shell, compositor, window manager, font/input   |
  +---------------------------------------------------------+
  | User services: init, permissions, networking, packages  |
  +---------------------------------------------------------+
  | Userspace API + stable IndianOS syscall ABI             |
  +=================== USER / KERNEL =======================+
  | IndianOS modular monolithic kernel                      |
  | Scheduler | VFS | IPC | process model | security        |
  | PMM / VMM | interrupts | timers | syscalls | handles    |
  +---------------------------------------------------------+
  | Native drivers: framebuffer, serial, PCI, virtio, ...   |
  +---------------------------------------------------------+
  | x86-64 CPU / ACPI / APIC / UEFI-provided boot metadata   |
  +---------------------------------------------------------+
```

## Automatic processor detection, issue #124 (HD0 foundation)

Every supported x86-64 kernel boot now probes the boot CPU using
**read-only CPUID**, rejects missing mandatory long-mode/NX/SSE2 flags, and
reads **only** the 16-byte PCI configuration headers on legacy bus 0
through the read-only 0xCFC data port (with 0xCF8 address selection).
The output is a versioned, no-heap `hardware_inventory::Inventory`
that can also be consumed by a future installer via
`installer_preflight`. The serial report distinguishes CPU vendor,
family/model, discovered GPU/NPU **hardware** counts, unsupported
drivers, missing physical QPU, and always-available CPU fallback.

This discovery NEVER enables DMA, maps GPU BARs, loads proprietary
drivers, formats disks, downloads packages or automatically connects
to remote quantum services. A GPU or NPU PCI class is *detected*, not
*usable*. Physical QPU detection remains unavailable until an
authenticated hardware/provider backend is implemented; the existing
local quantum simulator must not be represented as QPU hardware.
If PCI probing fails, boot logs a safe fallback and continues
without enabling drivers. QEMU boot acceptance validates the report,
and host tests validate CPU/PCI parsing, unsupported CPUs and fallbacks.

**Limits:** This is HD0, **not a finished graphical installer** or full
boot-time device manager. No installation UI is wired yet, only a reusable
read-only preflight API. PCI enumeration is intentionally scoped to legacy
x86 bus 0; ACPI MCFG multi-segment PCIe, bridge traversal, USB and
hotplug are HD1/HD2 backlog items. ARM64 and RISC-V need separate
ports; the present kernel still targets x86-64. Hardware features
such as AVX are reported but NEVER activated without full OS XSAVE/XCR0
and context-switch support. Production signed GPU/NPU drivers and
remote/real QPU discovery remain future work.

## Boot sequence

```text
UEFI firmware
  -> Limine loads IndianOS ELF (bootloader is third-party)
  -> limine protocol requests and responses
  -> kernel entry (initially one CPU; interrupts controlled)
  -> validate responses and memory map
  -> initialize serial logger + panic routine
  -> framebuffer status screen (if advertised and usable)
  -> physical memory manager (later)
  -> virtual memory manager / own page tables (later)
  -> x86-64 GDT, IDT, exceptions, interrupts, clock (later)
  -> process scheduler and syscall boundary (later)
  -> virtual filesystem, storage and initramfs (later)
  -> ring-3 init + services (later)
  -> desktop compositor + applications (much later)
```

**Do not assume firmware services remain available after exit from UEFI boot services.** The bootloader handles pre-kernel firmware and memory handoff; the kernel must use only the specific protocol-provided capabilities.

## Kernel module boundaries

- `arch/x86_64`: CPU startup, exception handlers, GDT/IDT, page-table primitives, ACPI/APIC handling.
- `boot`: validated, versioned boot info abstraction. No other module consumes Limine structs directly.
- `mm`: page-frame allocator; map/unmap; kernel heap; copyin/copyout.
- `sched`: tasks, timers, preemption, context switching; later SMP.
- `proc`: per-process address spaces and credentials.
- `syscall`: dispatcher, ABI version, capability/handle validation.
- `fs`: VFS plus read-only initramfs; persistent filesystems later.
- `ipc`: typed IPC or kernel primitives for services.
- `security`: process isolation, permissions and attack-surface reduction.
- `drivers`: probe/init/interrupt/DMA interfaces.

## Memory model

Start with bootloader-provided mappings; **never** assume all physical RAM is directly accessible without a verified HHDM. Track reserved/kernel/bootloader-reclaimable memory from the boot map. PMM owns free frames, VMM owns page tables, and the heap uses VMM. Avoid reclaiming boot metadata before copying required values. Default kernel policy should favour non-executable writable pages and explicit user/supervisor separation once own page tables are installed.

## Concurrency and safety

Early milestones: single CPU, no scheduler, no threads. Next: correct exception/interrupt paths, timer, cooperative tasks, then preemption and SMP. Keep interrupts disabled during early initialization where appropriate, and avoid locks before there are multiple execution contexts. Verify ABI, stack alignment, and floating-point/SIMD assumptions at kernel entry.

## Desktop boundary

The desktop is **not** part of the kernel. A user-space compositor gets graphics buffers and input events through designed kernel interfaces and draws windows. A stable compositor interface, localization and accessibility precede any attempt at app-store or Windows-program compatibility.

## Deferred decisions

A custom bootloader, ARM64, Wi-Fi on physical laptops, GPU acceleration, Windows ABI compatibility, full-disk encryption, secure boot chain, package signing and installers are separate projects. Track them by ADR and gate them behind tested kernel milestones.

## Planned cross-platform application and AI layer

The independent kernel exposes IndianOS process, memory, file, device and syscall primitives. These are **not** identical to Linux, Windows or macOS ABIs. A future user-space Interoperability Manager chooses a verified execution route for each app: native package, browser/PWA, implemented Linux ABI, Windows-API translation, authorized guest VM, Android runtime, or authorized remote Mac host. Each route requires separate engineering, licensing and testing.

An optional assistant helps a user express intent and understand choices; a deterministic package planner and signed, least-privilege installer implement the action after explicit approval. The language model cannot run privileged commands. See [COMPATIBILITY.md](COMPATIBILITY.md) and [AI_APP_ASSISTANT.md](AI_APP_ASSISTANT.md).
