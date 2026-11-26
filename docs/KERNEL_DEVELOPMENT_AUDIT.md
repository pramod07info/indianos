# Kernel development audit and pending-work ledger

Audit date: 26 Nov 2026. Pinned main: `1ccffb103d36d372edb0a2dba03636c92885caf8`.
Its production parent is merge `10abb92a2f09dff9c7c478248684310e3ced65ed`;
the later commit archives screenshots only. This is a source-and-evidence audit,
not new kernel functionality, a replacement backlog or release certification.

## What percentage can actually be measured?

**Total kernel code completion: not reliably measurable from this repository.**
The earlier approximately 40% experimental-kernel estimate was subjective and is
not promoted to a measured result by this audit. The architecture baseline K01–K10
also includes networking, security and release obligations beyond the M1 rows.
No agreed effort/risk weights or complete release acceptance denominator exists.
Counting lines, commits, modules or passing tests cannot supply that denominator.

The canonical M1 index has 99 detailed rows. Exactly 65 belong to kernel-focused
phases P1–P6: 10 boot, 12 memory/CPU, 12 process/IPC, 12 storage, 10 driver framework,
9 device-support rows. P8–P10 remain binding OS/service/desktop requirements but
are excluded from this specifically named kernel-focused coverage denominator.

| Evidence classification | Rows | Meaning |
| --- | ---: | --- |
| B — bounded guest evidence |30| A meaningful named capability has a real guest path and retained emulator evidence; **not** that the whole row or release requirement is complete. Opt-in evidence is identified as such. |
| P — partial runtime or model |20| Existing live foundations/helpers must be extended or integrated; do not rewrite them from unchecked labels. |
| R — runtime path not verified |13| The audited source does not establish the required live implementation. Helper models may exist; absence of proof is not proof no code exists anywhere. |
| H — physical qualification pending |2| Selected-machine receipts are missing. Other rows also have hardware or review obligations. |

**Bounded emulator evidence coverage: 30/65 =46.15%. This is not total kernel
completion, not effort-weighted progress and not production readiness.** It is
an auditable row classification. All 65 rows retain pending work or qualification.
The classification is reviewable judgment at broad row granularity, not a new
technical metric to inflate by splitting tasks. The existing formal task statuses
are unchanged; zero formally reconciled detailed rows does not mean zero code.

The checked-in M1 checklist remains 5/13 (38.46%). The owner reviewed/merged G4
and its [owner sign-off](https://github.com/pramod07info/indianos/pull/816#issuecomment-7186850863)
is recorded. Adopting that scoped decision in the canonical acceptance process
would make 6/13 (46.15%); this audit does not silently perform that transition,
fabricate an independent GitHub review or accept broader P9 requirements.

## Verified receipts and limits

All 13 Actions runs on production merge `10abb92` passed. Key receipts:
- [Full kernel boot](https://github.com/pramod07info/indianos/actions/runs/45774731073): actual paging/process/exception/preemption, legacy networking and other existing guest regressions; a passing opt-in profile does not activate that feature in normal desktop.
- [Combined reliability](https://github.com/pramod07info/indianos/actions/runs/45774731332), including [normal desktop job139755536097](https://github.com/pramod07info/indianos/actions/runs/45774731332/job/139755536097): all three jobs pass.
- [Exact-merge desktop artifact15096907358](https://github.com/pramod07info/indianos/actions/runs/45774731332/artifacts/15096907358) and [runtime artifact15096016906](https://github.com/pramod07info/indianos/actions/runs/45774731332/artifacts/15096016906).
- [Security S0](https://github.com/pramod07info/indianos/actions/runs/45774731484) and [durable VFS](https://github.com/pramod07info/indianos/actions/runs/45774731215) pass.

The actual normal-desktop log records original-client PID4 replacement,
load/unready/repeated replacement failures, physical input/pixel comparisons,
COW cuts/cold reboots and shutdown-exhaustion PASS. The earlier e7f06b7
shutdown-exhaustion failure was resolved and its oracle retained. These receipts
are not physical-machine, SMP scheduler, general TLS or arbitrary-driver proof.
No new kernel build, QEMU run or hardware test was needed or claimed for this
documentation audit; the linked existing logs were inspected.

## Allocation follow-up after the pinned audit

PR #819 merged as `01bbdec9207bb87ad86ad2006cdfb04d5c72a19b` adds the
fallible compositor-specific buffer. The next owned-page-heap slice extends
that production path with aggregate kernel-owner quotas and separate syscall
scratch ownership; see [allocation behavior and remaining work](KERNEL_FRAME_ALLOCATION.md).
PR #821 merged as `3fac00e5b4f3e3790b27ba007ac4154971e5bc01` adds owned
page allocations and per-owner quota accounting. The next slice adds shared
small-object scratch arenas, coalescing and production descriptor/row reuse.
P2-004/P2-011 remain unfinished. The classifications/counts above describe the
original pinned audit; this follow-up does not retroactively change its evidence,
formal task/gate statuses or completion percentage.

## Next implementation/qualification priorities

Keep M1 completion first. Reinspect the cited source before each coding task;
these are pending deltas to existing canonical IDs, not additional counted tasks.

1. **P4-008:** expose the existing descriptor sync route through the native ABI, with interrupted/error and durability acceptance; do not create another filesystem.
2. **P4-009:** general conflict-safe/blocking locks and stable file identity for broader writers; preserve the existing verified unrelated-reader witness.
3. **P4-006/010:** named-user ACL administration, timestamps and persistent user quotas; retain owner/group/mode enforcement.
4. **P3-003/011/012:** general spawn/exec/wait/parent supervision and lifecycle/resource-pressure guest stress; reuse current protected image and retirement paths.
5. **P2-004/011:** kernel allocation/heap and OOM/quota policy with actual recovery and conservation evidence.
6. **P2-006:** integrate demand paging/COW into the real fault handler with reference ownership and fail-atomic teardown.
7. **P2-010/P3-001/009:** FPU/SIMD context, real threads and wait/wakeup semantics before broader CPU scheduling claims.
8. **P2-008/009/P3-004:** AP run queues, per-CPU state, IPIs/TLB synchronization and fairness; AP acknowledgment alone is not SMP scheduling.
9. **P5-004/005/006/P6-005:** normal native networking and IRQ/DMA/IOMMU ownership/reset; preserve the isolated legacy virtio demo.
10. **P6-006/007 and P1-010/P6-009:** actual AHCI/NVMe/xHCI drivers followed by a documented safe physical qualification program. Never write physical disks without the separate installer/recovery authorization.

Additional architecture obligations: K06 needs general socket/interface/DHCP/
TCP lifecycle and timeout acceptance; a fixed exchange demo is insufficient.
K07/P1-009 needs a firmware trust chain, key revocation and trusted recovery.
P5-001/009 needs live ACPI power/reset qualification. TLS belongs in a validated
user service/library where appropriate, not an invented kernel feature count.
Broader P8–P10 desktop/service tasks, other filesystem formats and release/hardware
criteria remain visible in the original ledger; this kernel audit does not delete
or mark them optional by omission.

## Per-row pending ledger

The machine-readable copies live under `development_audit` on the existing 65
IDs in `M1_TASK_REGISTRY.json`. Source paths below are under
`rust/kernel_core/src/`. All classifications describe inspected production entry
points and their limits; existing helper modules/tests must be reused where valid.

### P1

| Existing task | Inspected source/symbol | Current bounded behavior | Remaining task and acceptance | Class |
| --- | --- | --- | --- | --- |
| M1-P1-001 — x86-64 UEFI boot protocol | `boot.rs`, `limine_requests.rs`, `kernel_main.rs`; `boot_validate` | UEFI/Limine normal and recovery boot execute in QEMU. | Qualify the documented supported firmware/platform set. **Exit:** Normal and serial recovery boot on exact source plus selected UEFI PCs. | B |
| M1-P1-002 — Pinned Limine bootloader | `Makefile`, `boot.rs`; `boot_validate` | Makefile pins and checks the Limine archive SHA256. | Preserve reproducible bootloader input and qualify updates. **Exit:** Checksum rejection and exact-image boot; Makefile LIMINE_SHA256 is the pin. | B |
| M1-P1-003 — Kernel ELF loading and entry validation | `program.rs`, `user_image.rs`; `plan_elf64`, `indianos_user_image_load` | Bounded ELF plans and real isolated service images load. | Broader application loading/exec and malformed-image stress. **Exit:** Reject malformed/overlapping/W+X ELF; real image load and reclamation. | B |
| M1-P1-004 — UEFI memory map validation | `boot.rs`, `pmm.rs`; `boot_memory_region_at`, `pmm_initialize` | Boot validates memory spans; allocator excludes reserved frames. | Platform-specific MMIO/DMA and firmware-map qualification. **Exit:** Bad map/span rejection and reserved-region/physical boot checks. | B |
| M1-P1-005 — Serial logging/panic/framebuffer status | `boot.rs`, `interrupts.rs`; `boot_draw_status`, `x86_64_exception_dispatch` | Serial diagnostics, panic/exception context and framebuffer status exist. | Persistent crash records and physical failure diagnostics. **Exit:** Real exception/CR2 log and serial-only recovery when graphics fails. | B |
| M1-P1-006 — Kernel stack and boot options | `interrupts.rs`, `user_runtime.rs`; `kernel_stack_layout` | Guarded kernel/trap stacks and bounded startup/shutdown budgets exist. | Complete boot-option policy and stack-pressure qualification across supported profiles. **Exit:** Bad option rejection, nested fault and worst-case stack acceptance. | P |
| M1-P1-007 — Validated initramfs boot modules | `initramfs.rs`, `boot.rs`; `indianos_initramfs_validate`, `boot_initramfs_file` | Validated archive supplies actual normal service images. | Preserve ownership/lifetime and malformed-media coverage. **Exit:** Missing/truncated/bad service archive refuses startup without media writes. | B |
| M1-P1-008 — Boot-failure/recovery mode | `kernel_main.rs`, `user_runtime.rs`; `kmain` | Missing/unready/crashed services enter bounded read-only recovery. | Broader platform recovery and formal release review. **Exit:** Startup, replacement load/unready/repeat and shutdown-exhaustion negatives. | B |
| M1-P1-009 — Verified/Secure Boot integration | `kernel_main.rs`, `limine_requests.rs`; `kmain` | Signed-package and boot-epoch fixtures are not firmware verified boot. | Integrate an actual UEFI trust chain and trusted recovery/key lifecycle. **Exit:** Reject an altered kernel/archive using real enrolled firmware trust. | R |
| M1-P1-010 — Normal startup on selected physical UEFI PCs | `kernel_main.rs`, `boot.rs`; `kmain` | Emulator receipts exist; selected-PC acceptance receipts were not supplied. | Inventory and qualify explicitly supported physical UEFI machines. **Exit:** Recorded firmware/CPU/display/storage boot and recovery matrix per machine. | H |

### P2

| Existing task | Inspected source/symbol | Current bounded behavior | Remaining task and acceptance | Class |
| --- | --- | --- | --- | --- |
| M1-P2-001 — GDT/IDT/TSS exception handling | `interrupts.rs`; `x86_64_exceptions_init` | Real GDT/IDT/TSS and protected trap-stack paths execute. | Physical and sustained nested-fault qualification. **Exit:** Actual #UD/#PF/user fault containment with intact guards. | B |
| M1-P2-002 — Physical frame management and reclamation | `pmm.rs`, `user_image.rs`; `pmm_allocate_frame`, `pmm_release_frame` | PMM-backed images allocate and reclaim real frames. | Memory-pressure/fragmentation soak and cross-core ownership. **Exit:** Repeated load/fault/reap with frame conservation; physical stress. | B |
| M1-P2-003 — Virtual memory and protected mappings | `vmm.rs`, `paging.rs`, `user_runtime.rs`; `vmm_map_page`, `paging_clone_release` | Owned mappings, CR3 switching, teardown and protected peer roots exist. | SMP TLB shootdown, broader mapping lifecycle and shared-memory policy. **Exit:** No stale/foreign mappings after real teardown, plus cross-CPU invalidation. | B |
| M1-P2-004 — Kernel allocation/heap policies | `lib.rs`, `pmm.rs`; `pmm_allocate_frame` | Core identifies itself as allocation-free; frame allocation is not a general heap. | Integrate a bounded kernel heap with allocation failure and ownership policy. **Exit:** Allocate/free/coalesce under pressure without leaks, deadlock or unsafe recovery. | R |
| M1-P2-005 — NX/W^X/guard page policies | `user_image.rs`, `syscall.rs`, `gui.rs`; `indianos_user_copy_writable` | W^X/NX and real guard/peer page checks protect service mappings and canvases. | Broader hostile-app and platform security qualification. **Exit:** Execute/write/guard/foreign-root negatives in actual guest address spaces. | B |
| M1-P2-006 — Demand paging/COW/page faults | `paging.rs`, `user_runtime.rs`; `x86_64_user_syscall_dispatch` | Address-space cloning and fault retirement do not prove fault-driven COW. | Integrate demand faults, COW refcounts, permissions and failure rollback. **Exit:** Fork/shared-page write splits correctly; allocation failure cannot leak aliases. | R |
| M1-P2-007 — High-resolution timer/APIC routing | `timer.rs`, `user_runtime.rs`; `x86_64_scheduler_apic_timer_dispatch` | R024 proves bounded single-core APIC timer preemption. | General timer API/deadline and multicore clock/interrupt qualification. **Exit:** Sustained preemption, monotonic accounting and timer failure tests. | B |
| M1-P2-008 — Multicore/SMP scheduler | `limine_requests.rs`, `user_runtime.rs`; `launch_one_parked_qemu_ap` | AP entry/CPUID acknowledgment exists; the test halts before PMM/user tasks. | AP run queues, per-CPU state, IPIs and synchronized scheduling. **Exit:** At least two CPUs execute isolated user tasks with correct cross-core teardown. | R |
| M1-P2-009 — Interrupt-safe atomics/locks/deadlock qualification | `retirement_gate.rs`, `smp_lifecycle_fence.rs`; `RetirementGate` | Single-BSP non-reentrant admission and lifecycle fences exist. | General IRQ-safe lock ordering/deadlock and SMP integration. **Exit:** Contention/interrupt/reentry/cross-CPU stress with no owner spinning on itself. | P |
| M1-P2-010 — FPU/SIMD and CPU topology context | `user_runtime.rs`, `hardware_inventory.rs`; `Task`, `inspect_cpu` | GPR context and CPU feature discovery exist; SIMD save/restore path not verified. | FPU/SIMD context ownership, enabled-state management and affinity. **Exit:** Two preempted users retain distinct SIMD/FPU states on supported CPUs. | R |
| M1-P2-011 — Memory-pressure/OOM policies | `pmm.rs`, `user_image.rs`; `pmm_allocate_frame` | Bounded allocation failure cleanup exists. | System-wide OOM, per-process memory quotas and pressure recovery. **Exit:** Quota isolation and repeatable exhaustion/recovery with frame conservation. | P |
| M1-P2-012 — Suspend/resume memory restoration | `paging.rs`, `kernel_main.rs`; `paging_clone_release` | Normal teardown/recovery is not suspend/resume restoration. | Define and implement firmware sleep, CPU/device and memory resume lifecycle. **Exit:** Resume preserves mappings and devices after failed and successful sleep. | R |

### P3

| Existing task | Inspected source/symbol | Current bounded behavior | Remaining task and acceptance | Class |
| --- | --- | --- | --- | --- |
| M1-P3-001 — Process and thread control blocks | `lib.rs`, `user_runtime.rs`; `Process`, `Task` | Fixed process PCBs and service tasks execute. | Independent thread objects, thread groups and general thread lifecycle. **Exit:** Create/schedule/join/terminate threads without resource or identity leakage. | P |
| M1-P3-002 — User-mode protected address spaces | `user_image.rs`, `paging.rs`; `indianos_user_image_load` | Services have separate CR3s, owned pages and guard mappings. | General dynamic address spaces and broader hostile-user soak. **Exit:** Foreign/kernel memory access fails while peers continue. | B |
| M1-P3-003 — Create/load/run/reap processes | `lib.rs`, `user_runtime.rs`; `indianos_process_create`, `discard` | Actual service create/load/run/exit/reap and PID4 replacement are tested. | General spawn/exec/wait rather than fixed-role bootstrap only. **Exit:** Repeated arbitrary image lifecycle with bounded cleanup and no zombies. | B |
| M1-P3-004 — Preemptive multicore fairness and priorities | `lib.rs`, `user_runtime.rs`; `Scheduler`, `indianos_scheduler_next` | Round-robin and opt-in single-core preemption exist. | Production priorities/fairness, SMP and per-CPU run queues. **Exit:** CPU-bound and blocking tasks meet documented fairness under multicore load. | P |
| M1-P3-005 — Versioned syscall ABI and safe user-copy | `syscall.rs`, `user_runtime.rs`, `user_image.rs`; `indianos_syscall_validate` | Versioned bounded descriptors and safe owned user-copy paths execute. | Stable complete native ABI coverage, migration/version and interruption semantics. **Exit:** Bad pointers/versions/ranges reject before side effects across supported operations. | B |
| M1-P3-006 — Capabilities and authenticated handles | `fs_handles.rs`, `capability.rs`, `user_runtime.rs`; `FileHandles`, `authenticated_file_identity` | Real file/IPC operations use kernel-authenticated process epochs and handles. | Broader delegation/revocation stress across dynamic process/service lifecycle. **Exit:** Stale/foreign rights fail after exit, rebind and namespace changes. | B |
| M1-P3-007 — Blocking/asynchronous IPC and shared memory | `ipc.rs`, `user_runtime.rs`; `IpcRouter` | Bounded mailbox send/receive, waits and service IPC are live. | General shared memory, pipe/event and asynchronous cancellation lifecycle. **Exit:** Cross-process backpressure, shared-page revocation and interrupted waits. | P |
| M1-P3-008 — Signals and cancellation | `process_events.rs`, `user_runtime.rs`; `ProcessEvent` | Lifecycle event queues exist; a general signal/cancellation delivery route is unverified. | Native signals or explicit equivalent with safe interrupted-operation semantics. **Exit:** Delivery/masking/cancel/exit races cannot corrupt process or storage state. | R |
| M1-P3-009 — Wait queues and futex-like facilities | `ipc.rs`, `user_runtime.rs`; `IpcRouter` | Cooperative IPC receive continuations and timeout waits execute. | General scheduler wait queues and futex-like thread synchronization. **Exit:** Lost wakeup, timeout, exit and cross-CPU wakeup race acceptance. | P |
| M1-P3-010 — Resource limits and thread synchronization | `fs_handles.rs`, `pmm.rs`; `FileHandles` | Fixed slot/transfer budgets and service write limits exist. | Per-process resource limits and full thread synchronization. **Exit:** Independent exhaustion does not starve or corrupt peers; synchronized teardown. | P |
| M1-P3-011 — Parent-child service ownership | `service_supervisor.rs`, `user_runtime.rs`; `ServiceTable`, `DesktopLifecycle` | Fixed desktop/service ownership and bounded restarts exist. | General parent-child hierarchy, orphan adoption and dynamic supervision. **Exit:** Parent crash/exit and child restart preserve authority and reap resources. | P |
| M1-P3-012 — Fault recovery and adversarial stress acceptance | `user_runtime.rs`, `process_events.rs`; `discard`, `DesktopLifecycle` | Guest fault/recovery and bounded crash cases pass. | Sustained adversarial lifecycle concurrency and resource-pressure soak. **Exit:** Long-run mixed fault/exit/restart/revoke tests with conserved resources. | P |

### P4

| Existing task | Inspected source/symbol | Current bounded behavior | Remaining task and acceptance | Class |
| --- | --- | --- | --- | --- |
| M1-P4-001 — Block-device abstraction | `block.rs`, `ata_pio.rs`; `BlockDevice` | Bounded block/window abstraction and real ATA errors/flush forwarding exist. | Asynchronous completion/cancellation and additional device transports. **Exit:** Timeout/reset/cancel and buffer ownership acceptance on actual block I/O. | B |
| M1-P4-002 — GPT/MBR partition discovery | `m1_partition.rs`; `discover`, `inspect_partition_receipt` | MBR/GPT discovery, mirrors, checksums and selected partition isolation execute. | Selected physical-media and broader mount lifecycle qualification. **Exit:** Corrupt/overlap/duplicate IDs fail closed and neighboring sectors remain unchanged. | B |
| M1-P4-003 — Hierarchical on-disk file/directories | `hier_vfs.rs`, `durablefs.rs`; `HierVfs`, `DurableFs` | Bounded hierarchical names/directories persist across cold boots. | General filesystem capacity, stress and metadata semantics. **Exit:** Deep/bounded traversal, remount and corrupt namespace recovery. | B |
| M1-P4-004 — VFS/mount/descriptor integration | `fs_handles.rs`, `hier_vfs.rs`; `FileHandles`, `selected_path_operation_on_device` | Live selected mounts, namespaces, descriptors and generation checks execute. | Dynamic multi-mount and broad process namespace lifecycle. **Exit:** Cross-process permissions, stale mount/revision and remount/revoke tests. | B |
| M1-P4-005 — Create/read/write/rename/delete/truncate | `fs_handles.rs`, `hier_vfs.rs`; `dispatch_mounted`, `create` | Native/path CRUD, truncate and positioned operations are live. | General capacity/concurrency/error/interruption matrix for the complete API. **Exit:** Real independent clients perform operations and verify bytes after reboot. | B |
| M1-P4-006 — Persistent user ownership/ACLs and timestamps | `fs_permissions.rs`, `hier_vfs.rs`; `Permissions`, `chmod_as` | Persisted owner/group/mode permissions and revocation exist. | Named-user ACL administration, timestamps and broader per-user acceptance. **Exit:** Persist/revoke/search/destination denial across rename, remount and process reuse. | P |
| M1-P4-007 — Metadata journaling/COW transaction | `durablefs.rs`; `DurableFs` | Checksummed COW data/metadata publication and recovery execute. | In-sector tears, controller-cache behavior and real hardware qualification. **Exit:** Interrupt each publication boundary; recover only valid old/new state. | B |
| M1-P4-008 — Page cache, fsync and flush semantics | `block.rs`, `hier_vfs.rs`, `fs_handles.rs`; `BlockDevice::sync`, `HierVfs::sync` | Durable flush/barrier and descriptor fsync implementation exist. | General page cache and native fsync routing/cancellation semantics. **Exit:** Dirty cache ordering and native sync errors survive crashes without false success. | P |
| M1-P4-009 — Concurrent locking and file sharing | `fs_handles.rs`; `selected_foreign_leases_current`, `FileHandles` | Range locks and verified unrelated-reader sharing exist; service is serialized. | Blocking/fair locks and broader concurrent writers with stable file identity. **Exit:** Lock/cancel/exit and overlapping/disjoint writers under actual concurrency. | P |
| M1-P4-010 — Quotas and general free-space allocation | `durablefs.rs`, `fs_handles.rs`; `DurableFs`, `FileHandles` | Fixed file extents/slots and service-wide write budgets exist. | General free-space allocator and persistent per-user quotas. **Exit:** Fragment/full/reclaim and per-user denial under crashes and remount. | P |
| M1-P4-011 — Removable devices and filesystem encryption | `block.rs`, `m1_partition.rs`; `BlockWindow` | Media identity retirement and safe mounts do not supply encryption/hotplug. | Removable-device lifecycle and actual encrypted filesystem/key handling. **Exit:** Removal during I/O, reinsert/stale handles, authenticated encrypted-data recovery. | R |
| M1-P4-012 — Reboot/powercut/corrupt-metadata regression | `durablefs.rs`, `fs_handles.rs`; `mount_recovery_readonly` | Real QEMU command-boundary power cuts, namespace corruption and cold readbacks pass. | In-sector torn writes and controlled physical power/cache-loss evidence. **Exit:** Old/new-only recovery through repeated cold boots and unchanged partition guards. | B |

### P5

| Existing task | Inspected source/symbol | Current bounded behavior | Remaining task and acceptance | Class |
| --- | --- | --- | --- | --- |
| M1-P5-001 — ACPI tables and firmware inventory | `batch3_acpi.rs`, `hardware_inventory.rs`; `parse_rsdp`, `parse_sdt` | Bounded ACPI/topology parsers and CPU/PCI discovery exist. | Live firmware table mapping, complete inventory and power-management integration. **Exit:** Real validated firmware inventory and safe shutdown/restart on selected machines. | P |
| M1-P5-002 — PCI enumeration on selected QEMU devices | `hardware_pcie.rs`, `hardware_inventory.rs`; `scan`, `discover` | Real bounded read-only PCI bridge/device enumeration executes. | Active ECAM/other segments, device lifecycle and broader hardware set. **Exit:** Guest root-port topology and supported physical enumeration without unsafe writes. | B |
| M1-P5-003 — MMIO/PIO access validation | `boot.rs`, `hardware_pcie.rs`; `console_framebuffer`, `scan` | Framebuffer spans and actual PCI/PIO transports validate bounded resource addresses. | General per-driver resource claims and malicious-device isolation. **Exit:** Bad/overlapping MMIO/PIO and resource revoke tests on activated drivers. | B |
| M1-P5-004 — APIC/MSI interrupt resource routing | `timer.rs`, `input.rs`, `hardware_pcie.rs`, `user_runtime.rs`; `x86_64_scheduler_apic_timer_dispatch`, `enable_desktop_irqs` | Live APIC timer and PIC input routing exist. | General MSI/MSI-X/shared IRQ resource ownership and interrupt-driven NIC integration. **Exit:** Reset/unregister/stale IRQ tests and supported-device interrupt delivery. | P |
| M1-P5-005 — Bounce DMA and IOMMU enforcement | `virtio_net.rs`, `block.rs`; `probe`, `BlockDevice` | Legacy QEMU NIC has bounded DMA buffers; validated helper models exist. | Actual IOMMU mapping/enforcement, IRQ/DMA reset and cross-device ownership. **Exit:** Unauthorized/stale DMA cannot access peer frames; reset waits for completion. | P |
| M1-P5-006 — Driver probe/start/stop lifecycle | `hardware_pcie.rs`, `virtio_net.rs`, `ata_pio.rs`; `scan`, `AtaPio` | Selected drivers probe and stop/retire individually. | Unified scoped driver start/quiesce/reset/release lifecycle. **Exit:** Failed/partial probe and reset revoke IRQ/DMA/MMIO without leaked ownership. | P |
| M1-P5-007 — Driver crash isolation | `user_runtime.rs`, `virtio_net.rs`; `Task` | User-process isolation is not driver isolation; current hardware drivers run privileged. | Real driver fault containment and resource-safe recovery. **Exit:** Faulting driver cannot corrupt kernel/peers or leave DMA/IRQ authority active. | R |
| M1-P5-008 — Hotplug handling | `hardware_pcie.rs`, `block.rs`; `scan`, `BlockDevice` | Static enumeration and stale-media rejection exist. | Live bus/device hotplug, removal and safe re-probe lifecycle. **Exit:** Concurrent unplug during I/O revokes resources and rejects late completions. | R |
| M1-P5-009 — Input/display/storage power and readiness | `input.rs`, `ps2_mouse.rs`, `ata_pio.rs`; `prepare_desktop_controller`, `AtaPio` | Readiness, session reinitialization and bounded I/O retirement exist. | ACPI/device power state and physical shutdown/resume qualification. **Exit:** Repeated actual shutdown/restart and input/storage recovery on supported PCs. | P |
| M1-P5-010 — Diagnostic/error reporting | `kernel_main.rs`, `ata_pio.rs`; `kmain`, `AtaPio` | Serial stage/fault/ATA diagnostics and CI artifacts exist. | General persistent crash/audit records and system observability. **Exit:** Retained diagnosable failures without log corruption, secret leakage or unbounded growth. | P |

### P6

| Existing task | Inspected source/symbol | Current bounded behavior | Remaining task and acceptance | Class |
| --- | --- | --- | --- | --- |
| M1-P6-001 — Serial UART/console | `console.rs`, `input.rs`; `poll_physical_byte` | Actual serial console and read-only recovery execute. | Supported physical UART and sustained error handling. **Exit:** Normal/recovery logging and UART-negative desktop control acceptance. | B |
| M1-P6-002 — PS/2 keyboard and mouse | `input.rs`, `ps2_mouse.rs`; `ScanDecoder`, `Mouse` | Actual PS/2 IRQ input, focus/capture and loss handling execute. | Supported physical keyboard/mouse and broader transport qualification. **Exit:** Real device input, held-button recovery, decoder loss and no stale editor input. | B |
| M1-P6-003 — UEFI-provided linear framebuffer | `gui.rs`, `boot.rs`; `compositor_commit`, `console_framebuffer` | UEFI framebuffer and validated complete PID4 frame copy execute. | Supported display modes/formats and physical framebuffer qualification. **Exit:** Exact pixel, clipping, stale/incomplete frame and real-display acceptance. | B |
| M1-P6-004 — Legacy ATA scratch disk driver | `ata_pio.rs`; `AtaPio` | Bounded legacy ATA PIO, IDENTIFY and flush/error handling execute. | Selected physical disks and real cache/power loss; never autoformat. **Exit:** Transport fault, corruption, flush and controlled reboot/power recovery. | B |
| M1-P6-005 — VirtIO network sample driver | `virtio_net.rs`, `network.rs`; `probe`, `run_demo` | R028 legacy QEMU PCI ARP/ICMP/UDP/DNS/TCP exchange exists. | Normal-boot network service and modern IRQ/DMA, sockets and interface lifecycle. **Exit:** Actual multi-connection/timeouts/IRQ/reset tests; do not enable legacy sample on real PCs. | B |
| M1-P6-006 — AHCI/NVMe production driver | `batch3_ahci.rs`, `batch3_nvme.rs`; `validate_command`, `Queue` | AHCI descriptor preflight and NVMe queue models are not live controller drivers. | Hardware initialization, DMA ownership, interrupts, flush and recovery. **Exit:** Real emulated then supported physical AHCI/NVMe reads/writes/reset/durability. | R |
| M1-P6-007 — USB/xHCI keyboard/mouse stack | `input.rs`, `ps2_mouse.rs`; `ScanDecoder`, `Mouse` | PS/2 paths do not establish USB/xHCI support. | Host controller, enumeration, rings and HID report delivery. **Exit:** Real xHCI keyboard/mouse, disconnect/reset and hostile descriptor tests. | R |
| M1-P6-008 — Modern display/GPU devices | `hardware_pcie.rs`, `gui.rs`; `scan`, `compositor_commit` | GPU discovery plus CPU framebuffer presentation is not GPU acceleration. | Validated device commands, memory/resource isolation and reset. **Exit:** Actual supported GPU backend plus preserved software fallback. | R |
| M1-P6-009 — Hardware inventory across selected physical PCs | `hardware_inventory.rs`, `hardware_pcie.rs`; `inspect_cpu`, `scan` | Emulated discovery exists; selected-PC driver receipts were not supplied. | Publish and qualify a physical CPU/IRQ/input/display/storage/NIC matrix. **Exit:** Exact machine/firmware/device IDs and independent negative/recovery receipts. | H |

PR #823 merged as `cfdcfab34a2178785360fcbabc64ecf0edba9c57`, adding shared
small-object scratch arenas. The next allocation slice adds bounded quarantine
recovery and classified pressure diagnostics. P2-004/P2-011 remain open pending
the remaining implementation and end-to-end acceptance requirements described in
[KERNEL_FRAME_ALLOCATION.md](KERNEL_FRAME_ALLOCATION.md).

## PR #825 expanded implementation boundary — 26 Nov 2026

The owner requested the following ten workstreams in the same open integration
PR. This is a pending-work ledger, not ten completed features or new acceptance.
Reuse the canonical task IDs and implementations above; do not create duplicate
allocator, scheduler, VFS, supervisor or controller models.

| Requested workstream | Existing implementation to extend | Remaining implementation/acceptance |
| --- | --- | --- |
| Heap, OOM and memory quotas | `kernel_heap.rs`, `kernel_arena.rs`, `user_image.rs`; P2-004/011 | Noncontiguous heap backing and bounded pressure recovery now implemented in #825. Broader consumers, total/shared RSS, global OOM and SMP admission remain. |
| Demand paging and COW | `paging.rs`, `vmm.rs`, `user_runtime.rs`; P2-006 | Fault-driven mappings, reference ownership, atomic write splitting and rollback with real user faults. |
| Multicore scheduling, IPIs/TLB | `smp_lifecycle_fence.rs`, `timer.rs`, `user_runtime.rs`; P2-008/009 | Actual AP run queues, remote wakeup/shootdown acknowledgements, race and teardown acceptance. |
| FPU/SIMD context | `fpu_context.rs`, `cpu_entry.rs`, `arch/x86_64/*entry.S`, `user_runtime.rs`; P2-010 | This slice implements eager BSP x87/MMX/SSE context boundaries. Exact-head guest evidence, preemptive register retention, AVX/XSAVE and migration remain unaccepted. |
| Threads, futexes and signals | `ipc.rs`, `user_runtime.rs`; P3-001/009 | General thread ownership, wait/wake/timeout/cancellation races and signal delivery. |
| Spawn/exec/wait and supervision | `user_image.rs`, `user_runtime.rs`, `service_supervisor.rs`; P3-003/011/012 | Arbitrary approved-image lifecycle and parent/wait semantics beyond fixed desktop roles. |
| Native fsync, blocking locks, ACLs/quotas | `fs_handles.rs`, `syscall.rs`, `durablefs.rs`; P4-006/008/009/010 | Descriptor fsync now wired below; interrupted durability acceptance, blocking lock queues and persistent named-user administration/accounting remain. |
| Normal networking and DMA ownership | `network.rs`, process retirement and PCI inventory; P5-004/005/006, P6-005 | Normal driver binding, IRQ/DMA/IOMMU lifetime and reset/error acceptance. |
| AHCI/NVMe and USB/xHCI | `batch3_ahci.rs`, `batch3_nvme.rs`, hardware inventory; P6-006/007 | Existing descriptor/queue models must become real initialized controllers; DMA, interrupts, completion, reset, flush and devices. |
| Verified boot, ACPI and physical qualification | boot handoff, hardware inventory and power primitives; P1-009/010, P5-001/009, P6-009 | Firmware trust/revocation/recovery chain and selected physical-machine power/device receipts. |

### Eager x87/SSE ownership implementation

Each normal desktop Task owns a 16-byte-aligned 512-byte FXSAVE64 context.
Preparation initializes empty x87/MMX registers, zero SSE payloads and masked
architectural control defaults, including on compositor replacement. Client
contexts remain in their existing Tasks during compositor-only recovery.

The admitted BSP configures CR0.MP/NE and CR4.OSFXSR/OSXMMEXCPT, clearing EM/TS.
Missing x87/FXSR/SSE/SSE2, enabled OSXSAVE, enabled AMD FFXSR or enabled interrupts
refuse admission. No AVX state is advertised or discarded silently. All context
and helper/global addresses must preserve their supervisor mapping in each
process root before readiness.

Raw entry/resume saves kernel state and restores that Task. Syscall, PIT/APIC
timer, keyboard/mouse and terminal fault entry save live user state and restore
kernel state before Rust executes. Direct interrupt returns restore user state
only for a CPL3 frame; an IRQ interrupting kernel code must not acquire user
state. Scheduling/exit paths retain the already saved Task context. Historical
profiles leave the context pointer null and keep their existing behavior.
This remains single-BSP ownership, not per-CPU storage or multicore scheduling.

Three host cases cover size/alignment/defaults, CPU-state admission and fresh
process payloads. Existing IRQ assembly tests link the real production helpers.
A separate `DESKTOP_FPU_ACCEPTANCE=1` image enables the user fixture feature,
which gives each PID distinct x87 values, XMM0/XMM15 and rounding controls, then
checks them across four yields. The additional QEMU harness requires all four
receipts before desktop readiness and retains serial/screenshot artifacts.
Ordinary normal-desktop acceptance still runs against the fixture-free image.
The fixture is cooperative scheduling evidence, not proof of preemption, AVX,
physical devices, general threads or migration. Two host evidence-oracle cases
reject missing/duplicate/late receipts. GitHub runs all builds/tests; none was
executed locally. All formal gate counts and completion estimates are unchanged.

### Native descriptor fsync integration (P4-008)

Native syscall 33 now admits descriptor operation 27 and advertises additive
feature bit 64 through the existing version-1, 64-byte ABI query. The descriptor
must contain only a live handle: path, argument, pointer, length and reserved
bytes are zero. The existing authenticated fsync backend enforces current
process incarnation, namespace, selected volume and write authority, verifies
metadata and invokes the actual driver barrier. It returns the verified durable
generation, or the existing BAD result on failure. CLOSE remains cleanup rather
than an implicit durability guarantee. Shutdown may repeat a nonmutating fsync
but continues refusing new opens and writes.

Desktop broker positioned writes now require fsync before reporting success;
Files proves a read-only handle cannot issue it. The normal QEMU oracle requires
exactly one fresh flush receipt before the positioned-write receipt, preserved
reader/cursor evidence and successful graphical commit. Negative controls reject
missing, duplicate or reordered flush evidence. Native ingress guard/crossing
checks now cover operation 27. Existing backend fsync success/argument/permission
and selected shared-reader tests are reused; a new failed-barrier case proves
writer revocation without charging a mutation. No local tests/builds were run.
Exact-head GitHub/QEMU results and interruption/power-loss qualification remain
necessary; this does not complete blocking locks, ACL administration or quotas.

### Reserved-stack demand paging (P2-006)

Normal desktop preparation now retains the top stack page and defers the other
reserved stack leaves after the existing full-footprint quota check. Deferred
slots retain their virtual addresses and remain charged in page_count and the
original image/private-launch budget. They are reservations, not resident RSS.
The stack must fit in one 2-MiB leaf-table span; its top resident leaf pins every
intermediate table borrowed by the running process root. No active PML4 child
can disappear during this conversion or during demand allocation.

A recoverable user page fault must be vector 14, CS 0x2b, and exactly a user
not-present read/write error (4 or 6). The active Task, running PCB, image pointer,
authority and CR3 must agree under BSP/IRQ-disabled retirement admission. Only
an owned deferred stack slot is eligible. A fresh PMM page is recorded before
access, zeroed, installed user/RW/NX, then invalidated in the active CPU TLB.
Protection, NX, guard, reserved-bit, foreign mapping and kernel faults retain
termination behavior. Failed allocation never publishes a mapping. A refused
rollback remains in a Task-owned physical frame receipt, retried before image
or PCB reclamation; it cannot be mistaken for an app pixel guard.

The raw exception entry now saves all 15 GPRs, supplies the unchanged vector/
error/RIP/CS/RFLAGS context with ABI stack alignment, and restores registers and
x87/SSE state only after successful recovery. It discards vector/error and clears
only CPU-generated RF before IRET. Every unrecovered exception still takes the
existing Rust terminal/fault isolation path. The legacy C implementation is
unchanged; this is the normal Rust desktop path.

Four image tests cover zeroed NX demand allocation and conserved quota/frames,
guard/protection/foreign mapping refusal, OOM/failed rollback ownership, and
failed initial deferral cleanup. Four host assembly oracles exercise the actual
return stub with CLI/IRET substituted, including pointer/register/RF mutations.
Those oracles do not claim hardware exception execution. The separate opt-in
guest fixture explicitly reads one absent stack page and writes another for
each of the four PIDs while x87/XMM state is live, then yields four times. Its
harness requires actual kernel fault-recovery receipts before readiness plus
the existing per-PID state-retention receipts. Existing normal desktop, guard,
exception, reboot, persistence, input and recovery checks remain required.

No local builds/tests/QEMU were run. Exact-head GitHub results are pending.
This does not implement demand-mapped ELF segments, arbitrary heap/mmap growth,
copy-on-write/shared-page refcounts, remote TLB invalidation or general fork.
P2-006 and requested points 3, 5, 6, 8, 9 and 10 remain unfinished. No formal
milestone or percentage changes.

### Exact-root invalidation and two-CPU remap qualification (P2-008/009)

The native demand-fault return now uses a shared local invalidation primitive
that checks an aligned canonical page address, an exact current CR3 root and
masked interrupts before INVLPG. It invalidates the current CPU/PCID only; it
cannot authorize remote frame reuse or replace an IPI acknowledgement.

The existing isolated two-vCPU Limine test now performs actual remapped-page
readback before its lifecycle acknowledgements. BSP validates translations of
three page-aligned static page tables and two distinct static data pages, then
publishes a supervisor/RW/NX alias in an unused high-half PML4 slot. AP must
match the source CR3 and prime a real cached translation. BSP alternates that
leaf between the two physical pages for 64 generations. The real AP invalidates
locally and reads the new page before the existing release/acquire fence may
acknowledge. BSP waits for that exact receipt before issuing the next remap.
No table/data backing is released: it remains kernel-owned until both CPUs
halt, before PMM, tasks, devices or filesystems start.

An explicit stale-translation fixture omits AP INVLPG after priming. A read of
the prior physical page must refuse acknowledgement, leave the fence pending,
and prevent BSP success. Its oracle requires the real prime/read-refusal/fatal
ordering and forbids successful 64-generation receipts. A generic startup error
cannot pass this negative case. Positive acceptance requires all exact ordered
receipts without duplicates or refusal. Both guest transcripts are retained
as exact-head GitHub artifacts, including on failure.

Four Rust host cases cover invalidation admission and proof backing geometry;
three host oracle cases cover positive/negative receipt requirements. Existing
lifecycle-fence stale/failed-local-work tests are reused. Actual QEMU positive
and stale-control execution is required in the existing AP workflow. No local
build/test/QEMU execution was performed. Until exact-head runs pass, these are
implemented paths and pending qualification, not verified remote-TLB acceptance.

This advances point 3 but does not complete it. Normal boot still parks APs.
Per-CPU IDT/TSS, IPI routing, run queues, scheduler migration, concurrent PMM
ownership and production multi-CPU shootdown/reclamation remain unfinished.
Points 5, 6, 8, 9 and 10 remain in the pending ledger; no formal gate or completion
percentage changes.

### Native supervised-child query/wait (P3-003/011/012)

Syscall 40 now exposes QUERY (operation 0) and bounded WAIT (operation 1) for
PID1's kernel-configured service children. Parent authority comes from the real
active PCB/Task and current filesystem identity, never a descriptor-supplied
caller. Child PID and a nonzero process incarnation bind a wait to one loaded
image; a compositor replacement cannot satisfy an old wait. Normal desktop uses
its existing monotonic incarnation receipts; legacy VFS profiles receive a
checked monotonic runtime incarnation. The existing fixed role membership is
retained; this does not invent an arbitrary parent/child process tree.

Input descriptor (64 bytes, little endian):

| Offset | Value |
| --- | --- |
| 0 | u16 version 1; u16 size 64 |
| 4 | u32 target PID, restricted to configured children 2..TASKS |
| 8 | u64 incarnation; QUERY may use 0 to discover the current incarnation |
| 16 | u64 selection-round deadline; QUERY 0, WAIT 1..32 |
| 24 | u64 writable output address for exactly 64 bytes |
| 32 | 32 reserved zero bytes |

Output: version/size/PID at offsets 0/2/4, incarnation at 8, i32 process state
at 16, u32 flags at 20 (bit 0 resources reclaimed, bit 1 wait consumed), u64
exit status at 24 and fault vector at 32; offsets 40..64 are zero. CR3, kernel
pointers and fault addresses are never exposed. Status/fault values are zero
until terminal cleanup completes. Return 64 means a complete status copy, 0
means a wait timeout with unchanged output, and BAD is the existing u64::MAX
failure result. Unknown versions/operations, reserved bytes, source guard or
crossing ranges, invalid destinations, stale incarnations and repeated WAITs
are refused. QUERY remains nondestructive.

WAIT captures a private ring-3 continuation and excludes that caller from
scheduler selection while eligible peers run. Deadlines count cooperative
selection rounds, not elapsed time or fabricated hardware ticks. A complete
exit/fault wins at the deadline. Successful copy-out consumes the completion
once; copy failure, timeout and stale identity cannot consume it. The exact
BSP admission permit is explicitly dropped before the noreturn yield transition.
Parent retirement cancels its continuation before releasing image state. This
adds a real scheduler wait facility, not general threads, futexes or signals.

Seven module tests cover request geometry, cleanup gating, timeout/peer chance,
parent/child reuse, consumption, copy failure and fault-data isolation. Native
ingress tests cover complete descriptors and guard/crossing addresses. The
existing five-boot durable VFS guest now first times out while its child is
parked, then launches it, blocks until real exit/reclamation, verifies status,
and rejects replay. The child proves it cannot query supervisor status. Parent
also tests stale generation, reserved fields and an unmapped output guard.
The existing reboot/persistence harness requires these ordered live receipts.
No local build, test or QEMU execution was performed; exact-head cloud results
remain required before acceptance.

This advances point 6 and the bounded wait prerequisite for point 5. General
spawn/exec, arbitrary parentage, service restart policy beyond existing roles,
threads/futexes/signals, normal IRQ/DMA/IOMMU networking, AHCI/NVMe/xHCI drivers,
verified boot and physical power/device qualification remain unfinished. No
formal gate or completion percentage changes.

### Legacy NIC interrupt notifications and DMA retirement (P6-005)

The existing QEMU virtio-net driver now has an explicit exclusive-IRQ11
qualification profile (`R136_NIC_IRQ_TEST=1`, Cargo `test-r136-nic-irq`). The
ordinary polled network profile and normal desktop are separate. Normal Desktop
cannot combine this experimental personality through its existing build gate.

The driver validates the real PCI interrupt line/pin and COMMAND readback before
arming notifications. Its DPL0 IDT stub preserves all GPRs and arbitrary kernel
stack alignment. The ISR masks IRQ11 before acknowledging the device ISR and
slave/master PIC, counts genuine queue notifications, and never dereferences a
DMA ring, packet, process, scheduler or heap. Removed-device/config-change ISR
values fail closed. Only the exclusive pre-user BSP profile may remap the PIC
and select legacy interrupt routing; this is not a generic platform IRQ router.

Bounded one-shot STI/NOP/CLI windows let actual device interrupts run while the
queue consumer retains exclusive DMA ownership. Polling windows are rate-limited;
completed RX/TX descriptors force a notification window before consumption. The
ISR masks its source before EOI so a stuck shared line cannot indefinitely
starve CLI. Shared devices/IRQs remain unsupported. Driver close masks and revokes
IRQ notification admission before the existing verified reset, PCI bus-master
disable and PMM ring release. If quiescence/reset/identity is uncertain, DMA
backing remains retained. No interrupt handler can touch released ring storage.

Two host admission/status cases and one negative receipt-oracle case were added.
The existing network workflow preserves its original polled guest test, then
builds a separate IRQ image and requires real ARP/ICMP/UDP/DNS/TCP packets,
IRQ arming, a nonzero real interrupt count and verified masking/reset before
success. It retains the guest serial transcript as an exact-head artifact.
Host-side packet validation still runs through QEMU's isolated socket backend;
no host physical NIC, public network or disk is attached. No local build/test/
QEMU execution was performed; exact-head GitHub results remain pending.

This advances point 8 without completing normal networking, interrupt-driven
multiqueue processing, MSI/MSI-X, IOMMU mappings or physical device qualification.
Points 5 (general threads/futexes/signals), 6 (general spawn/exec/parentage),
9 (actual AHCI/NVMe/xHCI controllers) and 10 (firmware trust and physical power/
device acceptance) remain unfinished. No formal gate or percentage changes.
