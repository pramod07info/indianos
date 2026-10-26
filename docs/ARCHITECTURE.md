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

## Issue #124 HD1: real PCI bridge topology and bounded PCIe MCFG decoder

The next hardware detection slice improves on HD0's bus-0 scan. A
no-heap, read-only PCI-to-PCI bridge enumerator now follows the
**validated secondary bus numbers** of header-type-1 class-06/04
bridges, with a capped 32-bus work queue, a 256-bus visit bitmap,
and a maximum of 16 discovered GPU/NPU devices. Primary,
secondary and subordinate ranges are checked before visiting
children. Unconfigured bridges are safely skipped. Duplicate
bus routes, malformed headers, overflow and probe failure cause
a safe CPU-only fallback, not unconditional driver activation.

Every x86-64 boot writes the original HD0 CPU/GPU/NPU profile and
a second `HD1 PCI buses=... bridges=... secondary_GPU=...` report.
The latter allows a **real QEMU PCIe root-port + downstream virtio
GPU** acceptance test to prove discovery across an actual bus
bridge. The QEMU test checks at least one downstream GPU, that
the physical QPU remains unclaimed, that CPU software fallback
remains available and that **no GPU driver has been activated**.
These are PCI config-space reads only, not graphics acceleration.

`hardware_mcfg::decode` separately validates checksum-protected
ACPI **MCFG PCIe memory configuration descriptors**, including
bounded table size, reserved bytes, nonoverlapping physical
apertures, segment/bus range uniqueness, ECAM 1-MiB alignment
and safe 4-KiB function offsets. It deliberately only calculates
numeric ECAM addresses: actual ACPI boot-pointer validation,
MMIO mapping, IOMMU policy, PCIe segment scanning and bridge
hotplug are **not implemented**. Native discovery still uses
the legacy x86 CF8/CFC mechanism for reachable segment-0
buses. No firmware tables are dereferenced by the new parser.

**HD1 is not full installer support.** The installer shares the
versioned read-only CPU/device profile API, but the interactive
installer workflow and automatic signed-driver selection remain
pending. CPU-only boot must still work if an optional GPU/NPU
is missing. A QPU requires separately verified real hardware
or explicitly authorized remote provider access; the quantum
simulator is not physical QPU hardware. Neither QEMU virtual
GPU enumeration nor a future-oriented design proves physical
Intel/AMD/NVIDIA graphics driver support.

## Issue #124 HD2: signed GPU/NPU driver eligibility, **NOT** auto-activation

Every x86-64 boot re-reads read-only PCI revision and subsystem IDs for
hardware discovered by the HD1 bridge topology scan. The ordinary kernel
ALWAYS has an empty/default-deny driver catalog until an independently
authenticated platform trust service supplies a signed driver catalog.
Boot emits an auditable line:
`IndianOS HD2 driver catalog=unavailable; signed candidates=0;
GPU/NPU drivers=not activated; QPU=unavailable; CPU fallback=enabled`.
This is a real boot-time policy decision, not an installed GPU driver.

The no-allocator `hardware_driver_policy` module defines a fixed-capacity,
versioned catalog for eight driver manifests. Each entry binds GPU/NPU
type, exact PCI vendor/device and optional subsystem IDs, revision range,
kernel driver ABI, driver release/version and SHA256 digest, required CPU
features and requested MMIO/DMA capability bits. Invalid/ambiguous
entries, unrecognized driver privileges, noncanonical unused slots,
unsupported CPU features, wrong PCI type and absent catalog fail closed.
The most-specific applicable subsystem entry wins over a generic
vendor/device match, but every match is only `SignedCandidate` metadata:
**not** permission to execute a driver, DMA into guest memory or access a
PCI BAR.

With the existing opt-in M2 crypto feature, signed catalogs undergo
**real Ed25519 validation** against a caller-supplied, independently
authenticated publisher registry at its exact signer key epoch. The entire
canonical binary transcript is SHA256-bound to the Ed25519 signature,
including every identity, requested capability and unused slot. A trusted
monotonic minimum catalog generation rejects replay; revoked signing
keys, signature tampering and signer-epoch changes are denied. The
signed fixture private key exists **only in host tests**. There is NO
embedded hardware trust root or accepted catalog on production boot.

The kernel's read-only `preflight` is reusable by the future installer.
HD2 host tests check:
- no catalog = no candidate or driver activation, even when a GPU is present;
- exact signed PCI IDs/subsystem specificity and deterministic selection;
- tampered driver rights/hash/version, stale signer/epoch, revoked publisher;
- malformed metadata, duplicate IDs, ambiguous revisions and unknown
  accelerator classes are all rejected; and
- QEMU's normal and bridged-virtio GPU boot reporters refuse missing
  HD2 fail-closed status.

**Not yet implemented:** actual signed driver binary download/installation,
a persistent authenticated driver catalog, hardware-rooted secure boot key
provisioning, safe MMIO mappings, IOMMU/DMA isolation, GPU command-buffer
validation, NPU inference, physical QPU providers, ARM64/RISC-V drivers,
full PCIe ECAM platform mapping, installer graphical UI, and real physical
GPU hardware support. Those remain Issue #124 work.

## Issue #124 HD3A: verified driver-image staging and hardware-resource planning

HD3A extends the HD2 cryptographically signed GPU/NPU catalog with an
**independent SHA-256 check of actual driver bytes**. A caller with both
`INSTALL` and `DRIVER` privileges, bound to an exact PID and generation,
may stage up to 65,536 bytes in a caller-owned, preallocated RAM
quarantine. Staging validates that PCI bus/slot/function and hardware
class match the read-only enumerated function, the Ed25519-signed driver
catalog's signer epoch is current and its externally supplied monotonic
sequence floor is met, and actual source bytes match the manifest digest.
Rejected updates erase quarantine. Revocation, PID reuse, bad signatures,
hardware replacement and stale releases fail closed.

A **resource-planning** layer validates requested MMIO/DMA apertures
against trusted platform-provided allowed memory windows, 4-KiB
alignment, size limits and nonoverlap. DMA proposals additionally
require a nonzero IOMMU domain and a stated isolation-ready state. A
strict entry point revalidates the catalog signature and quarantined
bytes again before proposing a plan. Returned
`ResourcePlan.activated=false`: it is **NOT** a live hardware
capability. An arbitrary supplied readiness flag is not independent
proof of IOMMU enforcement. This code never configures an IOMMU, maps a
BAR, enables bus mastering or executes the driver.

Every ordinary kernel boot prints the HD3 fail-closed serial status:
`IndianOS HD3 driver image=not installed; MMIO=not mapped;
DMA=disabled; IOMMU=not attested; GPU/NPU=CPU fallback;
QPU=unavailable`. Both normal and bridged-virtio GPU QEMU smoke
tests enforce that real-boot message. Dedicated Rust host regressions
(with/without M2 Ed25519) cover staging, byte/image tampering, wrong PCI
hardware, revocation, resource bounds, PID generations, permission
expiration, absent IOMMU evidence, and quarantine erasure.

**HD3A limits:** No persistent signed driver repository, real TPM-backed
platform root, trusted hardware memory-map attestation, functional
IOMMU page tables, GPU command ring/queues, display mode setting or
actual framebuffer acceleration are implemented. The next slice
should bring up an isolated virtio-gpu resource/queue prototype in
QEMU and validate actual device command completion with CPU software
fallback. This HD3A change does NOT make generic physical GPU/NPU
drivers or real QPUs operational.

## Issue #124 HD3B.1: virtio-gpu PCI protocol groundwork (QEMU)

IndianOS now recognizes the **actual QEMU modern virtio-gpu PCI
device** (`1af4:1050`) on the validated HD1 bridge topology. The
no-allocation PCI configuration reader traverses bounded vendor
capability lists to require uniquely identified, non-overlapping
COMMON, NOTIFY, ISR and DEVICE configuration ranges, valid BAR
indices and notification multipliers, and rejects malformed pointers,
cycles, duplicates, I/O-BAR pretending to be MMIO and wrong device
identities. All these operations are read-only PCI configuration
accesses; they do **not** map BARs, negotiate virtio features, allocate
DMA buffers, program queues or issue hardware GPU commands.

The `hardware_virtio_gpu::Protocol` implements the proper bounded
virtio-gpu 2D wire commands: GET_DISPLAY_INFO, RESOURCE_CREATE_2D,
RESOURCE_ATTACH_BACKING, SET_SCANOUT, TRANSFER_TO_HOST_2D and
RESOURCE_FLUSH. It validates response types/lengths, framebuffer
dimensions, physical-buffer alignment and allocation bounds. Host
tests use an explicitly **mocked** queue transport to exercise the
command sequence, malformed device replies, rogue PCI capabilities
and geometry/descriptor rejections. A separate real QEMU/OVMF
acceptance boots with and without a virtio-gpu device behind a PCIe
root port to verify the native PCI capability probe and CPU fallback.

**HD3B IS NOT COMPLETED:** A working QEMU virtqueue backend must still
be implemented with a trusted physical memory map, actual MMIO mappings,
verified DMA ownership/IOMMU isolation, negotiated device features,
error recovery, and GPU control-queue completion. Only then may the
project claim a real QEMU-rendered pixel buffer, and separate hardware
driver qualification is required for physical GPUs. The serial message
`HD3B ... queue=disabled; GPU acceleration=not active` is deliberate.
The new PCI probe does not weaken HD3A's signed-driver and CPU fallback
requirements. Existing OS framebuffer drawing remains CPU-owned.

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

## Issue #124 HD3B.2: isolated virtio-GPU control-queue staging (not a driver)

The `hardware_virtio_queue` module is a **pure Rust no_std planning and
descriptor-accounting layer** for a future modern virtio-GPU control queue.
It is deliberately NOT wired into the early boot path. It consumes an HD3A
`ResourcePlan` (which is numeric data only) and a PCI capability snapshot,
checks the requesting process generation and PCI bus/device/function, and
rejects missing proposed IOMMU domains, MMIO or DMA apertures. It requires
virtio `VERSION_1` and `ACCESS_PLATFORM` features for *potential* isolation;
merely offering these bits is never proof of actual isolation.

Split-ring descriptor, available, and used buffers occupy separately
page-aligned, nonoverlapping checked DMA windows. Queue size must be a
power of two in 2..=256 and each ring has enough space for the virtio
split-ring structures. Notification offsets are range checked against
the discovered modern PCI notify capability. The fixed-capacity queue
model rejects overlapping request/response buffers, buffer reuse,
wrong PID-generation, unknown/oversized completion, and exhaustion;
driver or service teardown retires its pending numeric commands.

**No physical queue has been created or enabled.** No memory pages have
been pinned, mapped into IOMMU tables or exposed to a device. No PCI MMIO
or configuration registers are written, no virtqueue used-ring bytes are
read from real hardware, and no GPU-rendered pixels exist. The model's
`complete` operation validates caller-provided metadata only; it does not
authenticate a real GPU completion. Real transport integration remains
blocked on IOMMU driver evidence, signed driver execution, bounded pinned
page ownership, queue memory ordering and tests against live QEMU and
qualified physical hardware. CPU software rendering stays the fallback.

CI runs `hd3b2_` host tests both with/without the M2 feature and a
bare-metal `no_std` compile, in addition to existing full kernel and
QEMU acceptance. This is engineering groundwork, not HD3B final acceptance.
