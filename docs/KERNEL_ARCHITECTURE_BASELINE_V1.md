# IndianOS kernel architecture baseline v1 — review candidate

**Status:** PROPOSED; design review and architecture freeze **not completed**.
**Date:** 2026-10-28.
**Product target:** one independent, Rust-first kernel for Desktop, Developer
Workstation and Server editions. This is a design contract, not a statement
that the features below are implemented or safe on physical hardware.

## 1. Architectural decisions

- **Kernel form:** modular monolith initially, with privileged code restricted
  to hardware-critical mechanisms; prefer user-mode services for graphical
  shell, app store, package/update manager, networking policy and orchestration.
- **Implementation:** freestanding Rust `no_std` with small reviewed x86-64
  assembly/FFI entry units; no C translation units in the default production
  kernel. Existing C/Python references may remain for regression testing.
- **Supported platform first:** x86-64 UEFI / Limine under QEMU+OVMF. Real PCs
  need a qualified hardware matrix before safe physical installation. ARM64,
  Apple Silicon, GPUs and actual quantum hardware are independent later
  platform/device programs, **not** inherited capabilities.
- **Edition isolation:** Desktop / Developer / Server differ in user-space
  profiles, installed services, resource policy and UI; they do not fork the
  kernel. Headless boot must not require graphics.
- **Hard contract:** userspace never gets raw kernel pointers, physical
  memory, arbitrary MMIO, unrestricted DMA or unauthenticated driver access.

```text
  Desktop Home Canvas | Developer Workgrid | Server Ops Plane (optional)
             + secure CLI / apps / VM services
  ---------------------------------------------------------------
  Userland: PID1, service manager, network/security policy, files,
            package/update service, optional compositor, runtime APIs
  ---------------------------------------------------------------
  Versioned userspace ABI: syscall dispatch, handles, IPC, mmap, timers
  ===================== PRIVILEGE BOUNDARY =======================
  Rust-first modular-monolithic IndianOS kernel:
    process/thread/scheduler  | virtual memory and isolation
    capabilities/IPC          | VFS and block I/O
    interrupt/timers          | sockets transport primitives
    device bus and drivers    | crash logging and security hooks
  ---------------------------------------------------------------
  HAL: x86-64 QEMU first; separate ARM64 architecture port later
  UEFI -> pinned Limine -> validated boot metadata -> kernel -> PID1
```

## 2. Boot, hardware enumeration and edition handoff (K01)

- Define a versioned `BootInfo` with memory map, firmware addresses,
  framebuffer (optional), command line and hardware inventory, each with
  strict bounds, ownership and lifetime rules.
- Reject malformed mandatory boot responses and reserve firmware, MMIO,
  DMA and kernel regions before PMM initialization; serial-only recovery
  diagnostics must work without a GPU.
- Detect CPU capability and peripherals **before** driver matching; discovery
  is not permission to activate hardware. The edition profile is selected
  after kernel initialization by signed/trusted userspace configuration.
- Normal boot and recovery boot must have deterministic state transitions.
  Installer code must never wipe a disk without explicit informed consent.

## 3. Memory, processes and scheduling (K02)

- PMM owns physical frames; VMM owns page tables, mapping lifetimes, shared
  memory and W^X/NX protections; only validated user memory may be copied.
- Define explicit locking, lock ordering, interrupt-context rules, per-CPU
  state, kernel stacks, PID+generation identity, teardown/reclamation and
  OOM/back-pressure. Verify no use-after-free or cross-process mappings.
- Use preemptive scheduling with bounded critical sections; plan priority,
  timer accounting, fairness and SMP as independently testable extensions.
  Never claim SMP readiness because single-core QEMU preemption passes.

## 4. Userspace ABI, processes, IPC, and service lifecycle (K03)

- Version every public syscall structure, handle/capability right, event
  queue and IPC message; reject unsupported versions and oversized payloads.
- Establish stable semantics for spawn/exec/exit/wait, per-process namespace,
  file descriptors/handles, pipes/IPC, signals or explicit event substitutes,
  monotonic timers, memory mapping, threads, and service supervision.
- Define restart semantics: PID1 failure, service crashes and orphaned
  resources must not leave the kernel in an unrecoverable state.
- **Linux/POSIX translation is a future userland compatibility workstream**,
  not the native IndianOS syscall ABI.

## 5. Filesystem, block I/O, and crash recovery (K04)

- VFS handles namespace, permissions, path normalization, mount ownership,
  open/read/write/fsync/rename/unlink and verified per-process capabilities.
- Block layer handles bounded I/O, timeouts, flush ordering, device reset,
  error propagation and QEMU power-loss injection.
- Define partition discovery (MBR/GPT), signed/checksummed metadata,
  recovery, safe mount/read-only behavior and transactional update storage.
- FAT/exFAT, ext4, NTFS, APFS and others each require separate tested
  drivers or deliberate read-only access; VFS alone does **not** supply their
  formats. Do not claim universal filesystem write support.

## 6. Device driver contract and architecture portability (K05)

- Driver lifecycle: enumerate -> validate resource descriptors -> claim
  scoped resources -> initialize -> start -> quiesce -> reset/release.
- Each driver has explicit IRQ, DMA, MMIO, IOMMU (when available), buffer
  ownership, resource limits and crash/fallback policies.
- Initial regression devices: serial, PS/2, framebuffer, virtio block/net,
  supported QEMU storage. Real NVMe/USB/Wi-Fi/GPU drivers require a separate
  qualification program. GPU/NPU discovery is not acceleration.
- Architecture-specific code lives behind defined HAL/arch modules; ARM64
  requires boot, MMU, exception, interrupts, timers, SMP and device drivers
  plus separate tests; Mac support is not inferred from ARM64 alone.

## 7. Network and server foundations (K06)

- Define bounded sockets, DNS/DHCP support, multiple TCP connections,
  retransmission, listener/accept, UDP, routing, interface configuration,
  firewall hooks and failures/timeouts; TLS usually belongs in validated
  user-space libraries/services rather than privileged kernel code.
- Server mode must boot headless and host supervised applications only when
  storage, network and privilege boundaries are proven. Docker/Kubernetes
  compatibility additionally requires container/VM mechanisms and APIs:
  ordinary TCP demos are insufficient.
- Developer edition shares this kernel and uses developer tools in userspace.

## 8. Security boundaries and recovery (K07)

- Explicitly document attacker model: hostile user apps, malicious archives,
  crafted networks and filesystem images, untrusted devices, compromised
  update media and power loss.
- Enforce least privilege at every entry point, robust user-pointer copying,
  W^X/NX, protected secrets, quotas, audit and permission revocation.
- Signed code/packages/drivers, trusted boot, TPM measurements and recovery
  must be staged by platform with real evidence; no promise of 100% malware
  or hacker resistance.
- Design rollback for interrupted updates and corrupted filesystem metadata.

## 9. Graphics, virtualization, and compatibility boundaries (K08)

- Kernel exports protected input/display/buffer mechanisms. User-space
  compositor owns window policy, scene graph, surfaces and event routing.
- GPU backends require resource isolation and validated commands; retain
  CPU framebuffer fallback for supported installations.
- Guest virtualization needs CPU virtualization support, VM memory and device
  models; no promise of arbitrary Windows/macOS guest support or licensing.
- Linux/Windows/Android/macOS application compatibility is not a filesystem
  trick and must be qualified **per app**, with legal distribution routes.

## 10. Observability, reliability and performance budgets (K09)

- Document serial/persistent crash logs, panic policy, boot diagnostics,
  trace points, watchdog recovery, telemetry permissions and version tags.
- Define repeatable measurements: cold boot, scheduling latency, context
  switching, memory use, I/O flush latency, socket reliability and overhead
  with/without optional GUI services. Record baselines before setting SLAs.
- Regressions must execute at exact commit SHA in QEMU and later on an
  explicit physical-device certification matrix.

## 11. Build, CI, compatibility contracts and release governance (K10)

- Keep Rust unit, ABI, fuzz/negative and QEMU execution checks; gate merging
  on mandatory checks with reviewed branch protections.
- Record an ADR for each disputed contract (syscalls, driver ABI, scheduler
  semantics, file guarantees, security trust anchors, edition boot profile).
- No production feature is 'complete' from a compiled binary, a host-only
  unit test, a diagram, or a screenshot without interaction evidence.
- Use versioned interfaces and a documented migration policy; do not break
  existing verified QEMU behavior to make a design appear clean.
- One substantial, reviewable commit per coherent implementation batch,
  followed by integration-branch checks and final-main SHA verification.

## 11A. Optional edition profiles, shared kernel and resource policy

**Design target, not currently implemented.** Desktop, Developer Workstation,
and Server are userland feature profiles, while **All-in-One** is a combined
installer preset. Installing three profiles does not load three kernels.
Kernel interface contracts must support service/process isolation, optional
headless boot, resource accounting, quotas, shared on-disk packages and
rollback; none should require the kernel to know edition branding.

- A signed versioned manifest identifies required/optional packages, enabled,
  on-demand and disabled services, configuration migrations and dependencies.
- The effective set is a validated **union** of selected profiles; conflicts
  must be presented before installation, not silently overridden.
- Core kernel and mandatory security/recovery services cannot be disabled
  by a profile. GUI/compositor is optional; Server can boot headless.
- Installing a database/runtime/VM must **not** start it by default. A server
  can stay active in the background when user explicitly chooses always-on
  deployment, with status, limits and audit controls.
- Define scheduling/accounting interfaces for memory, CPU time, I/O and
  network quotas. Prefer safe admission/failure over unbounded resource
  exhaustion; full quota enforcement depends on later implementation.
- Profile activation/deactivation is a transaction using verified packages,
  dependency checks, user-data preservation, rollback and recovery. Profile
  switching must not change disk partitions automatically.
- Accept each preset and a full three-profile combination under the same
  kernel build with idle/loaded performance measurements and regressions.

See [edition profiles & All-in-One plan](EDITION_PROFILES_AND_ALL_IN_ONE.md)
and backlog UX08–UX09 / UX15–UX18. These requirements extend K02/K03/K06/K07
design reviews without changing current verified QEMU claims.

## 12. Architecture-freeze readiness checklist

Architecture v1 can be labeled **reviewed/frozen** only when:

- [ ] Decision records reconcile old C-first ADR 0001 with current Rust-first
      default and specify ownership for architecture changes.
- [ ] Boot/HAL, memory, scheduler, syscalls/IPC, VFS, drivers and network
      interfaces have versioned data-layout and failure-mode specifications.
- [ ] Security threat model and user/kernel/driver trust boundaries reviewed.
- [ ] Desktop, Developer and Server edition boot/service boundaries agreed.
- [ ] Dependency and risk matrix links every kernel contract to M1–M5 and
      to reproducible regression coverage.
- [ ] Review includes independent architecture/security signoff; open
      decisions and rejected alternatives are recorded.

**Not yet frozen.** This baseline establishes a structured review target,
not functional completion of the kernel or general hardware support.

## 13. Dependency order for implementation

1. K01–K03 stable boot, memory, processes and ABI.
2. K04 and K07 storage integrity and security gates.
3. K05–K06 device/network and headless hosting foundations.
4. K08 compositor boundary and compatibility/virtualization contracts.
5. K09–K10 system hardening, release and test evidence.

The M1–M5 delivery exits in `docs/DELIVERY_MILESTONES.md` remain
authoritative. The UX01–UX14 design-system backlog remains planned, with
implementation gated on relevant kernel and userspace contracts.
