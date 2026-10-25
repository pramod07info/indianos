# M1 integrated phase backlog — full mandatory scope

**Release target:** graphical desktop boots without test flags, users can run independent ring-3 Files/Terminal, and editing a document is still visible after reboot on a qualified supported machine.
**Required phases:** 1–4, 8–10, with minimum driver support from 5–6. All tasks below are in the **one M1 workstream**, NOT claimed complete by this one commit.

**Single commit is a review/change packaging policy, not a promise that an entire OS can be implemented and independently hardware-tested in one commit.**

## Existing evidence that must not be discarded
- Rust-first kernel, Limine/OVMF, process and syscall/IPC experiments and QEMU APIC tests are already in main.
- QEMU opt-in R031.1 verifies automatically started **three** actual ring-3 desktop processes.
- QEMU opt-in R031.9 verifies graphical Files, real emulated disk, two boots, input and screenshots.
- R027/R031 durable COW metadata and bounded HierVfs exist. No autoformat of real hardware is allowed.
- M1 adds bounded read-only MBR/GPT partition table discovery (GPT header and entry-array CRCs), rejects overlap/out-of-bounds media and explicit partition selection, with sector-isolated mounting of the existing durable VFS. No disk is formatted by discovery or mount.
- New M1 modules connect Terminal `ls`, `cat`, `write`, `mkdir`, and `mv` to the same durable HierVfs, editable input line, verified process identity and per-window damage notifications. Feature-gated Rust tests exercise remount, failed writes and stale identities.
- The CI workflow now runs M1 host/no_std tests AND the existing genuine R031.9 QEMU disk+desktop integration, rather than treating the host adapter test as a ring-3 success.

## All phase tasks and required completion state
### P1 Bootloader and kernel startup

- [ ] x86-64 UEFI boot protocol — Existing bounded/experimental implementation; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Pinned Limine bootloader — Existing bounded/experimental implementation; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Kernel ELF loading and entry validation — Existing bounded/experimental implementation; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] UEFI memory map validation — Existing bounded/experimental implementation; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Serial logging/panic/framebuffer status — Existing bounded/experimental implementation; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Kernel stack and boot options — Partially implemented; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Validated initramfs boot modules — Existing bounded/experimental implementation; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Boot-failure/recovery mode — Not implemented or not production-qualified; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Verified/Secure Boot integration — Not implemented or not production-qualified; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Normal startup on selected physical UEFI PCs — Not implemented or not production-qualified; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.

### P2 CPU, interrupts and memory

- [ ] GDT/IDT/TSS exception handling — Existing bounded/experimental implementation; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Physical frame management and reclamation — Existing bounded/experimental implementation; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Virtual memory and protected mappings — Existing bounded/experimental implementation; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Kernel allocation/heap policies — Partially implemented; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] NX/W^X/guard page policies — Partially implemented; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Demand paging/COW/page faults — Partially implemented; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] High-resolution timer/APIC routing — Existing bounded/experimental implementation; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Multicore/SMP scheduler — Not implemented or not production-qualified; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Interrupt-safe atomics/locks/deadlock qualification — Partially implemented; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] FPU/SIMD and CPU topology context — Not implemented or not production-qualified; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Memory-pressure/OOM policies — Not implemented or not production-qualified; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Suspend/resume memory restoration — Not implemented or not production-qualified; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.

### P3 Processes, syscalls, scheduling and IPC

- [ ] Process and thread control blocks — Existing bounded/experimental implementation; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] User-mode protected address spaces — Existing bounded/experimental implementation; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Create/load/run/reap processes — Existing bounded/experimental implementation; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Preemptive multicore fairness and priorities — Partially implemented; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Versioned syscall ABI and safe user-copy — Existing bounded/experimental implementation; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Capabilities and authenticated handles — Existing bounded/experimental implementation; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Blocking/asynchronous IPC and shared memory — Partially implemented; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Signals and cancellation — Not implemented or not production-qualified; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Wait queues and futex-like facilities — Partially implemented; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Resource limits and thread synchronization — Partially implemented; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Parent-child service ownership — Partially implemented; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Fault recovery and adversarial stress acceptance — Partially implemented; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.

### P4 Durable filesystem and storage

- [ ] Block-device abstraction — Existing bounded/experimental implementation; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] GPT/MBR partition discovery — Partially implemented; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Hierarchical on-disk file/directories — Existing bounded/experimental implementation; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] VFS/mount/descriptor integration — Partially implemented; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Create/read/write/rename/delete/truncate — Partially implemented; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Persistent user ownership/ACLs and timestamps — Not implemented or not production-qualified; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Metadata journaling/COW transaction — Existing bounded/experimental implementation; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Page cache, fsync and flush semantics — Partially implemented; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Concurrent locking and file sharing — Not implemented or not production-qualified; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Quotas and general free-space allocation — Not implemented or not production-qualified; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Removable devices and filesystem encryption — Not implemented or not production-qualified; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Reboot/powercut/corrupt-metadata regression — Partially implemented; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.

### P5 Required bus and device framework

- [ ] ACPI tables and firmware inventory — Partially implemented; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] PCI enumeration on selected QEMU devices — Partially implemented; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] MMIO/PIO access validation — Existing bounded/experimental implementation; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] APIC/MSI interrupt resource routing — Partially implemented; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Bounce DMA and IOMMU enforcement — Partially implemented; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Driver probe/start/stop lifecycle — Partially implemented; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Driver crash isolation — Not implemented or not production-qualified; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Hotplug handling — Not implemented or not production-qualified; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Input/display/storage power and readiness — Partially implemented; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Diagnostic/error reporting — Partially implemented; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.

### P6 Minimum supported physical/emulated drivers

- [ ] Serial UART/console — Existing bounded/experimental implementation; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] PS/2 keyboard and mouse — Existing bounded/experimental implementation; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] UEFI-provided linear framebuffer — Existing bounded/experimental implementation; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Legacy ATA scratch disk driver — Existing bounded/experimental implementation; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] VirtIO network sample driver — Existing bounded/experimental implementation; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] AHCI/NVMe production driver — Not implemented or not production-qualified; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] USB/xHCI keyboard/mouse stack — Not implemented or not production-qualified; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Modern display/GPU devices — Not implemented or not production-qualified; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Hardware inventory across selected physical PCs — Not implemented or not production-qualified; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.

### P8 PID1, system services and sessions

- [ ] Normal boot into userspace without opt-in test flag — Not implemented or not production-qualified; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Authenticated PID1 service supervision — Partially implemented; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Dependency-ordered startup — Partially implemented; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Watchdog, restart and failover — Partially implemented; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Logging/config/settings services — Partially implemented; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Users/groups/session auth — Not implemented or not production-qualified; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Clock/timezone/locale management — Not implemented or not production-qualified; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Background jobs and notifications — Not implemented or not production-qualified; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Storage/input/package/network service daemons — Partially implemented; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Shutdown/reboot/logout and suspend lifecycle — Partially implemented; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.

### P9 GUI/window system

- [ ] Ring-3 compositor separate from kernel — Not implemented or not production-qualified; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Per-process private pixel surfaces — Partially implemented; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Window operations and stacking — Existing bounded/experimental implementation; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Keyboard focus/mouse delivery from physical IRQ — Partially implemented; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Desktop shell/taskbar/launcher — Partially implemented; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Unicode font/layout engine — Not implemented or not production-qualified; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Theme/icon/wallpaper controls — Partially implemented; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Graphics acceleration and software fallback — Not implemented or not production-qualified; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Multi-display/DPI scaling — Not implemented or not production-qualified; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Clipboard/drag-drop/notification UI — Not implemented or not production-qualified; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Compositor restart and sandbox isolation — Not implemented or not production-qualified; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] QEMU image/input acceptance and hardware displays — Partially implemented; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.

### P10 Essential desktop applications

- [ ] Graphical login and lock — Not implemented or not production-qualified; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Functional GUI Files CRUD explorer — Partially implemented; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Interactive graphical Terminal commands — Partially implemented; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Settings application GUI — Partially implemented; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Task Manager/monitor — Not implemented or not production-qualified; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Signed app installer GUI — Not implemented or not production-qualified; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Browser/PWA runtime — Not implemented or not production-qualified; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Text editor/document viewer — Not implemented or not production-qualified; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Media/image/audio apps — Not implemented or not production-qualified; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Network/display/sound settings — Not implemented or not production-qualified; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Clock/calculator/screenshot utilities — Not implemented or not production-qualified; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Help, backup/recovery interface — Not implemented or not production-qualified; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.

## Exact M1 exit gate (ALL required, not achieved by this commit)
1. A *normal* installed OS boots via UEFI into a properly isolated ring-3 window server and two independent graphical applications.
2. A keyboard and pointer actually trigger Terminal and Files actions without COM1/host automation controlling the functionality.
3. In the guest, Files/Terminal create, read, edit, rename and delete documents on a durable disk with validated ownership/capabilities.
4. A shutdown and boot on the **same** scratch storage verifies the last committed file bytes; injected torn writes recover correctly.
5. A process crash and restart cannot reuse stale handles, credentials or graphics surface grants.
6. Selected supported devices pass CPU/memory/IRQ/input/display/storage diagnostics in QEMU and designated real PCs.
7. Code is independently reviewed; full exact-commit QEMU tests, host unit tests, no_std and real-hardware QA evidence all pass.

## Integration note
M1.1 is currently an actively evolving draft branch with one squashed feature commit, not all M1 completion. Subsequent work may be integrated into that same branch (replacing the single feature commit after checks are re-run), but only publish a release-complete claim after the above exit gate is independently satisfied.
