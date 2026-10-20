# IndianOS application compatibility architecture

**Status:** design target, not implemented on IndianOS. Absolute compatibility with every Windows, Linux, macOS or Android application cannot be promised.

## Goal: broad interoperability with explicit capability detection

IndianOS has its own kernel and system ABI. Linux, Windows, Android and macOS software depends on APIs, ABIs, graphics stacks, device drivers, CPU instruction sets and licenses that IndianOS does not magically inherit. Different execution modes should be chosen on an app-by-app and device-by-device basis.

## Execution routes

| Route | Implementation strategy | Key dependencies / caveats |
| --- | --- | --- |
| Native | Compile and package for IndianOS ABI | Requires userspace, compiler SDK, stable application ABI |
| Web/PWA | Install approved, sandboxed web application | Requires browser/runtime, online and offline policy |
| Linux compatibility | Translate or implement selected Linux/POSIX syscalls | ELF loader, Linux ABI, signals, threading, filesystem semantics, Vulkan/OpenGL bridges; no guarantee |
| Windows compatibility | Implement or port Win32/NT user-space compatibility | PE loader, registry, Windows DLL/API semantics, graphics/audio; Wine cannot simply be copied unchanged onto a new kernel |
| Windows VM | Run a legitimately licensed guest OS in an isolated hypervisor | Hardware virtualization, storage, network, guest OS license and resources |
| Android runtime | Android runtime/container/guest | Android API, kernel-dependent features, licensing and GPU stack |
| macOS application | Native third-party port, web alternative or authorized remote Mac | Apple-specific frameworks, signing and licenses; do not claim local execution on generic PCs |

**No universal binary:** x86-64 and ARM64 use different machine code. Application compatibility must be tracked separately from *hardware portability*. Emulation or translated binaries may perform differently.

## Architecture: interoperability manager

    IndianOS AI / App Store / CLI
                 |
          App intent parser
                 |
         Trusted app catalog  <-- signature + version + source + license
                 |
         Compatibility planner <-- hardware / user permission / runtime inventory
                 |
    +------------+-----------------+------------------+
    | Native     | Compatibility   | VM / remote / web|
    | package    | runtime         | session          |
    +------------+-----------------+------------------+
                 |
          Security policy engine
                 |
         User-approved installation
                 |
          Health check + rollback

The **planner never executes instructions**. Runtime providers own execution, sandboxing and failure reporting; the privileged package manager owns installation. AI text cannot bypass this trust boundary.

## Compatibility registry

Each app must have a canonical identity, supported CPU architectures, architecture-specific route candidates, source provenance and license metadata, a device/driver requirement list, required runtime versions, signed-package information, compatibility test results (including regressions), and an explicit confidence state. Never infer full compatibility from an extension such as .exe or .app.

User-facing outcomes:

- **Verified:** tested on a supported IndianOS build and hardware/VM profile.
- **Experimental:** a runnable route exists but may fail.
- **Planned:** a future route is envisioned, not implemented.
- **Unsupported:** no tested route; explain why.
- **Unavailable:** requires hardware, licensing or resources not currently available.

The initial reference catalog in this repository contains invented *examples* and **no verified application support**.

## Safe progression

1. First implement kernel, process isolation, VFS, networking, graphics and native package manager.
2. Provide secure web apps and ports of open-source native tools.
3. Build one bounded Linux/POSIX ABI target and a compatibility test suite.
4. Evaluate Windows user-space API compatibility and licensed Windows VMs.
5. Add Android runtime only after required Linux/Android facilities exist or provide virtualization.
6. For macOS-only apps, prioritize web/portable ports or a compliant remote Mac service; no circumvention of licensing.
7. Add ARM64 kernel/hardware support as a separate architecture milestone, not a checkbox.

## Compatibility test harness (future)

Test with legal sample apps and recorded fixtures: install, launch, API/ABI behavior, render, storage, networking, accessibility, security containment, update, uninstall, and crash recovery. Measure application coverage as a tested percentage of a published app set, never as 100% of all global software.

## Legal and licensing

Honor proprietary SDK, OS guest, trademark and redistribution restrictions. Refer to actual vendor licenses, especially for Apple platforms. Do not bundle Windows/macOS files or binaries without authorization.

References:
- https://www.winehq.org/about/
- https://www.qemu.org/docs/master/about/emulation.html
- https://www.apple.com/legal/sla/
