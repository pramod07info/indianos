# IndianOS M1: tasks 001–090 across eight workstreams

Based on `docs/M1_PHASE_SCOPE.md`, master 99-task gate mapping and issue #616. Base main: `8f242484846c275d3976f5aaec4652bc419c4885` (PR #782 merged). These are **workstream ownership plans**, not eight launched coding agents or a claim that all 90 tasks have been implemented.

- Scope: master tasks 001–090 (the remaining 091–099 are **not** included in this batch).
- Gate numbers here are checklist positions 6–13, corresponding to issue G1–G8.
- Checkboxes remain OPEN until the full acceptance evidence is recorded at the exact SHA.
- Single integration commit policy; merged source, host tests, QEMU and physical qualification are separate obligations.

## Current source delta in this batch

1. `vfs_path.rs`: guarded rename now refuses renaming/replacing the namespace root, including canonical spelling aliases. Regression tests cover both directions.
2. `fs_permissions.rs`: added a bounded, conservative parent+ancestor permission policy for rename. **Not yet wired to the live VFS rename syscall**; requires authenticated credentials and mount generation revalidation.
3. `batch3_network_security.rs`: validated DHCP ACK refuses non-unicast lease/server addresses and ordinary subnet network/broadcast endpoints. Regression tests added. It remains policy parsing, **not an actual NIC driver or complete DHCP client**.

## Workstation ownership and all 90 source tasks

### Workstream 1 — Gate 6 (G1): Terminal + graphical Files · 1–4

- [ ] **001.** Create/read/write/rename/delete/truncate (phase ledger source #39; **acceptance pending**)
- [ ] **002.** Functional GUI Files CRUD explorer (phase ledger source #89; **acceptance pending**)
- [ ] **003.** Interactive graphical Terminal commands (phase ledger source #90; **acceptance pending**)
- [ ] **004.** Text editor/document viewer (phase ledger source #95; **acceptance pending**)

### Workstream 2 — Gate 7 (G2): GPT/MBR + ATA partition safety · 5–7

- [ ] **005.** Block-device abstraction (phase ledger source #35; **acceptance pending**)
- [ ] **006.** GPT/MBR partition discovery (phase ledger source #36; **acceptance pending**)
- [ ] **007.** Legacy ATA scratch disk driver (phase ledger source #60; **acceptance pending**)

### Workstream 3 — Gate 8 (G3): Boot, PID1 + supervised services · 8–30

- [ ] **008.** x86-64 UEFI boot protocol (phase ledger source #1; **acceptance pending**)
- [ ] **009.** Pinned Limine bootloader (phase ledger source #2; **acceptance pending**)
- [ ] **010.** Kernel ELF loading and entry validation (phase ledger source #3; **acceptance pending**)
- [ ] **011.** UEFI memory map validation (phase ledger source #4; **acceptance pending**)
- [ ] **012.** Serial logging/panic/framebuffer status (phase ledger source #5; **acceptance pending**)
- [ ] **013.** Kernel stack and boot options (phase ledger source #6; **acceptance pending**)
- [ ] **014.** Validated initramfs boot modules (phase ledger source #7; **acceptance pending**)
- [ ] **015.** Boot-failure/recovery mode (phase ledger source #8; **acceptance pending**)
- [ ] **016.** Process and thread control blocks (phase ledger source #23; **acceptance pending**)
- [ ] **017.** Create/load/run/reap processes (phase ledger source #25; **acceptance pending**)
- [ ] **018.** Parent-child service ownership (phase ledger source #33; **acceptance pending**)
- [ ] **019.** Normal boot into userspace without opt-in test flag (phase ledger source #66; **acceptance pending**)
- [ ] **020.** Authenticated PID1 service supervision (phase ledger source #67; **acceptance pending**)
- [ ] **021.** Dependency-ordered startup (phase ledger source #68; **acceptance pending**)
- [ ] **022.** Watchdog, restart and failover (phase ledger source #69; **acceptance pending**)
- [ ] **023.** Logging/config/settings services (phase ledger source #70; **acceptance pending**)
- [ ] **024.** Users/groups/session auth (phase ledger source #71; **acceptance pending**)
- [ ] **025.** Clock/timezone/locale management (phase ledger source #72; **acceptance pending**)
- [ ] **026.** Background jobs and notifications (phase ledger source #73; **acceptance pending**)
- [ ] **027.** Storage/input/package/network service daemons (phase ledger source #74; **acceptance pending**)
- [ ] **028.** Shutdown/reboot/logout and suspend lifecycle (phase ledger source #75; **acceptance pending**)
- [ ] **029.** Graphical login and lock (phase ledger source #88; **acceptance pending**)
- [ ] **030.** Help, backup/recovery interface (phase ledger source #99; **acceptance pending**)

### Workstream 4 — Gate 9 (G4): Ring-3 graphics + process isolation · 31–50

- [ ] **031.** Virtual memory and protected mappings (phase ledger source #13; **acceptance pending**)
- [ ] **032.** NX/W^X/guard page policies (phase ledger source #15; **acceptance pending**)
- [ ] **033.** User-mode protected address spaces (phase ledger source #24; **acceptance pending**)
- [ ] **034.** Resource limits and thread synchronization (phase ledger source #32; **acceptance pending**)
- [ ] **035.** Fault recovery and adversarial stress acceptance (phase ledger source #34; **acceptance pending**)
- [ ] **036.** UEFI-provided linear framebuffer (phase ledger source #59; **acceptance pending**)
- [ ] **037.** Ring-3 compositor separate from kernel (phase ledger source #76; **acceptance pending**)
- [ ] **038.** Per-process private pixel surfaces (phase ledger source #77; **acceptance pending**)
- [ ] **039.** Window operations and stacking (phase ledger source #78; **acceptance pending**)
- [ ] **040.** Desktop shell/taskbar/launcher (phase ledger source #80; **acceptance pending**)
- [ ] **041.** Unicode font/layout engine (phase ledger source #81; **acceptance pending**)
- [ ] **042.** Theme/icon/wallpaper controls (phase ledger source #82; **acceptance pending**)
- [ ] **043.** Graphics acceleration and software fallback (phase ledger source #83; **acceptance pending**)
- [ ] **044.** Multi-display/DPI scaling (phase ledger source #84; **acceptance pending**)
- [ ] **045.** Clipboard/drag-drop/notification UI (phase ledger source #85; **acceptance pending**)
- [ ] **046.** Compositor restart and sandbox isolation (phase ledger source #86; **acceptance pending**)
- [ ] **047.** Settings application GUI (phase ledger source #91; **acceptance pending**)
- [ ] **048.** Task Manager/monitor (phase ledger source #92; **acceptance pending**)
- [ ] **049.** Media/image/audio apps (phase ledger source #96; **acceptance pending**)
- [ ] **050.** Clock/calculator/screenshot utilities (phase ledger source #98; **acceptance pending**)

### Workstream 5 — Gate 10 (G5): PS/2, USB input + focus · 51–54

- [ ] **051.** Input/display/storage power and readiness (phase ledger source #55; **acceptance pending**)
- [ ] **052.** PS/2 keyboard and mouse (phase ledger source #58; **acceptance pending**)
- [ ] **053.** USB/xHCI keyboard/mouse stack (phase ledger source #63; **acceptance pending**)
- [ ] **054.** Keyboard focus/mouse delivery from physical IRQ (phase ledger source #79; **acceptance pending**)

### Workstream 6 — Gate 11 (G6): Persistent VFS + ACL/recovery · 55–61

- [ ] **055.** Hierarchical on-disk file/directories (phase ledger source #37; **acceptance pending**)
- [ ] **056.** Persistent user ownership/ACLs and timestamps (phase ledger source #40; **acceptance pending**)
- [ ] **057.** Metadata journaling/COW transaction (phase ledger source #41; **acceptance pending**)
- [ ] **058.** Page cache, fsync and flush semantics (phase ledger source #42; **acceptance pending**)
- [ ] **059.** Concurrent locking and file sharing (phase ledger source #43; **acceptance pending**)
- [ ] **060.** Quotas and general free-space allocation (phase ledger source #44; **acceptance pending**)
- [ ] **061.** Reboot/powercut/corrupt-metadata regression (phase ledger source #46; **acceptance pending**)

### Workstream 7 — Gate 12 (G7): Syscalls, IPC + authenticated handles · 62–67

- [ ] **062.** Versioned syscall ABI and safe user-copy (phase ledger source #27; **acceptance pending**)
- [ ] **063.** Capabilities and authenticated handles (phase ledger source #28; **acceptance pending**)
- [ ] **064.** Blocking/asynchronous IPC and shared memory (phase ledger source #29; **acceptance pending**)
- [ ] **065.** Signals and cancellation (phase ledger source #30; **acceptance pending**)
- [ ] **066.** Wait queues and futex-like facilities (phase ledger source #31; **acceptance pending**)
- [ ] **067.** VFS/mount/descriptor integration (phase ledger source #38; **acceptance pending**)

### Workstream 8 — Gate 13 (G8): Kernel and hardware qualification · 68–90

- [ ] **068.** Verified/Secure Boot integration (phase ledger source #9; **acceptance pending**)
- [ ] **069.** Normal startup on selected physical UEFI PCs (phase ledger source #10; **acceptance pending**)
- [ ] **070.** GDT/IDT/TSS exception handling (phase ledger source #11; **acceptance pending**)
- [ ] **071.** Physical frame management and reclamation (phase ledger source #12; **acceptance pending**)
- [ ] **072.** Kernel allocation/heap policies (phase ledger source #14; **acceptance pending**)
- [ ] **073.** Demand paging/COW/page faults (phase ledger source #16; **acceptance pending**)
- [ ] **074.** High-resolution timer/APIC routing (phase ledger source #17; **acceptance pending**)
- [ ] **075.** Multicore/SMP scheduler (phase ledger source #18; **acceptance pending**)
- [ ] **076.** Interrupt-safe atomics/locks/deadlock qualification (phase ledger source #19; **acceptance pending**)
- [ ] **077.** FPU/SIMD and CPU topology context (phase ledger source #20; **acceptance pending**)
- [ ] **078.** Memory-pressure/OOM policies (phase ledger source #21; **acceptance pending**)
- [ ] **079.** Suspend/resume memory restoration (phase ledger source #22; **acceptance pending**)
- [ ] **080.** Preemptive multicore fairness and priorities (phase ledger source #26; **acceptance pending**)
- [ ] **081.** Removable devices and filesystem encryption (phase ledger source #45; **acceptance pending**)
- [ ] **082.** ACPI tables and firmware inventory (phase ledger source #47; **acceptance pending**)
- [ ] **083.** PCI enumeration on selected QEMU devices (phase ledger source #48; **acceptance pending**)
- [ ] **084.** MMIO/PIO access validation (phase ledger source #49; **acceptance pending**)
- [ ] **085.** APIC/MSI interrupt resource routing (phase ledger source #50; **acceptance pending**)
- [ ] **086.** Bounce DMA and IOMMU enforcement (phase ledger source #51; **acceptance pending**)
- [ ] **087.** Driver probe/start/stop lifecycle (phase ledger source #52; **acceptance pending**)
- [ ] **088.** Driver crash isolation (phase ledger source #53; **acceptance pending**)
- [ ] **089.** Hotplug handling (phase ledger source #54; **acceptance pending**)
- [ ] **090.** Diagnostic/error reporting (phase ledger source #56; **acceptance pending**)

## Acceptance and merge requirements

- Real implementation and tests in the specified owner modules; do not credit policy helpers as working live drivers or UI features.
- Rust kernel-core host tests, freestanding no_std compile, memory/security regressions and static checks must pass on the final commit.
- Exact-head QEMU/OVMF normal Desktop boot, keyboard/mouse, persistent file mutation, cold reboot, corrupted-media recovery and process restart/permissions checks must pass with artifacts.
- For G8 physical drivers, SMP/interrupt/DMA/IOMMU/TPM/power loss, keep hardware acceptance OPEN until supported machines are qualified.
- Independent review, branch-protection and post-merge main checks are required. A PR merge alone does not complete M1.

Reference: [M1 completion issue](https://github.com/pramod07info/indianos/issues/616), [phase requirements](M1_PHASE_SCOPE.md), [G3–G8 acceptance](M1_GATES_8_10_ACCEPTANCE.md).


## Continuation: credential-checked live HierVfs operations

- `HierVfs::rename_dir_as`: checks writable/searchable source and destination parents and searchable ancestor chain, then executes the existing atomic durable tree rename.
- `HierVfs::rmdir_as`: authenticates directory removal without bypassing nonempty checks.
- `HierVfs::symlink_as`: atomically commits creator-owned permissions with the durable link body, never following its target.
- `HierVfs::unlink_symlink_as`: checks parent/leaf authority, removes the link only, and does not follow the target.
- New regression tests cover wrong-owner denial without checkpoint changes, self-descendant rename denial, persistence across remount, nonempty directory removal refusal, symlink owner metadata and unlink isolation.

**Scope restriction:** These are actual HierVfs Rust kernel operations and integration tests, not yet evidence of complete authenticated ring-3 live syscall wiring, physical disk durability, or eight running agents. All 90 task acceptance checkboxes remain pending.

## Continuation: authenticated Files directory enumeration and link inspection

- `HierVfs::list_dir_as` requires READ+SEARCH on the requested directory and SEARCH across its parent chain.
- `HierVfs::readlink_as` requires READ access on the link, does not follow the target, and is denied by an inaccessible parent.
- Additional regression verifies owner denial with untouched output, exact mixed file/symlink listing, unchanged generation on reads, and remounted symlink access.
- These are live HierVfs methods but not yet full ring-3 syscall/GUI integration or final gate acceptance.

## Continuation: selected-volume syscall32 routes to authenticated VFS

The real `fs_handles::selected_path_operation_on_device` now invokes `HierVfs::rmdir_as`, `rename_file_as`, `list_dir_as` and `rename_dir_as` for pathname actions 2, 7, 8 and 9. These calls retain the existing exact-process-generation admission, namespace guard, selected-partition receipt, write-budget, COW mount and quarantine handling. The extra authenticated VFS boundary check may increase metadata I/O; host/QEMU regressions on this precise revision are required.

This is live selected-volume syscall integration for these four action types, **not** a complete POSIX filesystem, proof of independent writable PID2, or gate G7 acceptance.

## Selected syscall32 owner-isolation/reboot regression

A new actual `FileHandles` selected-disk test creates a directory and file as authenticated PID1, proves PID2 cannot rename or modify the volume before device I/O, renames a nonempty directory via syscall32 action9, then verifies PID2's read and directory listing across a fresh selected-disk remount. This exercises real on-disk COW metadata through the kernel-core block-device acceptance harness; production QEMU and physical power-loss evidence remain separately required.
