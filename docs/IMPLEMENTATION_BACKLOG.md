# IndianOS actionable implementation backlog

Status captured 2026-10-23. This backlog tracks the path from an experimental
QEMU kernel to a usable OS. **An item is done only after its acceptance checks
are observed**. Source code, diagrams, and passing host tests alone do not
establish working interrupts, a desktop, or third-party application support.

## Kernel-first architecture design and integration gate (2026-10-28)

**Decision:** complete and review the kernel architecture baseline **before**
prioritizing three-edition UI implementation. This is design-first, not an
instruction to stop mandatory CI, safety fixes or current M1 kernel integration.
Target specification: [kernel architecture baseline v1](KERNEL_ARCHITECTURE_BASELINE_V1.md).
Existing verified QEMU foundations remain verified at their actual scope;
unimplemented contracts below remain **OPEN**.

| Workstream | Priority | Kernel architecture deliverable | Design acceptance |
|---|---|---|---|
| K01 | P0 | Boot handoff, CPU/firmware inventory and hardware abstraction | Versioned BootInfo, initialization state machine, ownership/validation, recovery boot and fallback contract |
| K02 | P0 | Memory, processes, thread scheduler and future SMP | PMM/VMM map ownership, kernel/user isolation, preemption, teardown, locking, OOM and multicore plan |
| K03 | P0 | Native syscall ABI, IPC, capabilities and PID1/services | Versioned syscall/handle protocols, process lifetime, IPC rights, crash/restart semantics and ABI compatibility policy |
| K04 | P0 | VFS, block storage and persistent recovery | Mount/path semantics, per-user rights, journaling/flush/rename guarantees, partition safety, explicit filesystem-driver scope |
| K05 | P0 | Driver lifecycle, DMA/IRQ contract and architecture ports | QEMU-first interface; resources/ownership, IRQ isolation, platform-specific x86-64 vs ARM64 extension plan |
| K06 | P0 | Reliable TCP/IP/socket API and headless service hosting | DHCP/DNS, listening sockets, multi-connection handling, timeouts, network security and Server profile contract |
| K07 | P0 | Threat model, permission gates and signed recovery chain | Untrusted input inventory; kernel/driver/app boundaries, rollback and auditable security acceptance |
| K08 | P1 | Graphics/compositor boundary and VM/compatibility contracts | User-space compositor, protected surface ownership, virtualization requirements, app-by-app compatibility limits |
| K09 | P1 | Kernel diagnostics, failure recovery and performance budgets | Panic/logging strategy, fault injection, metrics and reproducible QEMU/hardware benchmarks |
| K10 | P0 | Architecture review, ADR reconciliation and freeze criteria | Reconcile C-first ADR with Rust-first implementation, explicit open decisions, reviewed versioned contracts and M1–M5 test traceability |

**Kernel design gate:** K01–K10 architecture deliverables must receive
technical/security review with documented open decisions before an
architecture-freeze claim. 'Design approved' does not mean 'module coded'
or 'production-ready'. Implementation is a separate evidence-driven gate.

**Priority change for original UI backlog:** UX01–UX02 visual design
exploration may continue in parallel without touching kernel interfaces.
UX03 depends on K02/K03/K05/K07/K08; UX05–UX07 must not be claimed
production-ready until their underlying kernel, storage, networking,
security and userland service contracts pass. Three editions share **one
kernel**, with distinct user-space profiles, not three kernels.

## Verified foundations (not general-purpose production features)

- [x] UEFI/Limine x86-64 QEMU boot with a Rust-first no_std kernel
- [x] GDT/IDT/TSS, PMM/VMM, ring-3 fault isolation and basic syscalls
- [x] Two independent ring-3 programs using cooperative scheduling
- [x] Limited fixed-capacity IPC, capability, block, filesystem and
  interactive-console primitives with regression tests
- [x] Absolute VFS path normalization (R021)
- [x] R024 xAPIC timer preemption with genuine IRQ-to-context-switch proof in QEMU ([passing verification](https://github.com/pramod07info/indianos/actions/runs/39913743740))
- [x] Opt-in persistent QEMU serial shell with live worker launch, status, wait and cleanup ([passing verification](https://github.com/pramod07info/indianos/actions/runs/39924988234))
- [x] Dedicated QEMU IDE device identification and copy-on-write multi-sector storage with reboot and metadata-recovery proof ([verification](https://github.com/pramod07info/indianos/actions/runs/39929748026))
- [x] R028 QEMU legacy virtio-net PCI and strict ARP/IPv4 ping, UDP echo, DNS A answer and TCP request/reply ([passing QEMU network test](https://github.com/pramod07info/indianos/actions/runs/39933935521))
- [x] R029 actual 1280×800 QEMU framebuffer two-window composition, PS/2 mouse focus and keyboard move/minimize/restore/close with screenshot proof ([passing QEMU test](https://github.com/pramod07info/indianos/actions/runs/39941297124))
- [x] R029 verified ring-3 ELF drawing Files/Terminal content via bounded, copied and identity-checked surface commands ([passing QEMU screenshots](https://github.com/pramod07info/indianos/actions/runs/39949896007))
- [ ] R029 complete isolated user-space compositor, per-app surface ownership and event-driven application protocol (rendering and input broker remain kernel-owned)
- [x] R030 developer SDK staged signed packages with Ed25519 verification, semver update, rollback and QEMU execution of exported ELF (see [R030 package acceptance](R030_SIGNED_NATIVE_PACKAGES.md))
- [ ] On-device package database, runtime app launcher, per-user permissions and persistent device-backed installer
- [ ] Normal boot launching interactive userland by default and general service management

## Implementation sequence and exit conditions

| ID | Priority | Deliverable | Required exit evidence |
|---|---|---|---|
| R022 | P0 | Connect absolute VFS paths to TinyFs flat-root create/read/delete | Reject traversal and nested paths; preserve existing data; remount CRUD regression and no_std build |
| R023 | P0 | Bounded process-event FIFO with per-PID teardown | Zero/one-capacity, wraparound, overflow and ordering tests; no_std build |
| R024 | P0 | Calibrated local APIC preemption wired to scheduler | QEMU observes IRQs, capture, two CPU-bound tasks alternating without yield; memory/scheduler cleanup |
| R025 | P0 | **QEMU-verified** capability-gated process, IPC and flat-root file syscalls | **Verified:** ring-3 identity, rights, user-copy bounds, cross-PID denial, revocation and teardown; production service integration remains R026/R027 |
| R026 | P1 | **QEMU-verified** interactive init, restricted worker launch and lifecycle | **Verified:** serial QEMU commands, shell survives worker start/exit, process status/wait and cleanup. General service management remains future work |
| R027 | P1 | **QEMU-verified** ATA LBA28 identification and durable multi-sector volume | **Verified:** five QEMU boots, file overwrites, mirrored checksummed metadata, torn-copy recovery, strict partition bounds; generic PCI/AHCI/NVMe and production storage remain unsupported |
| R028 | P1 | **QEMU-verified** virtio-net PCI, ARP/IPv4/ICMP/UDP/DNS and bounded TCP demo | **Verified:** real QEMU guest Ethernet packets, host-validated checksums, ARP resolution, ICMP ping, UDP exchange, DNS A response, TCP handshake and reply; general sockets/routing remain pending |
| R029 | P1 | **QEMU graphics acceptance verified; user-space compositor pending** | **Verified:** actual 1280×800 VGA framebuffer pixels, two independent windows, QEMU PS/2 mouse clicks, keyboard focus/move/minimize/restore/close and screenshot differences. Ring-3 bounded draw-command API is now QEMU-verified; independent per-app surfaces and full user-space compositor remain unfinished |
| R030 | P1 | **QEMU-verified developer-host native signed package SDK** | Build two real static ELF64 apps, sign with Ed25519, reject tampering/downgrades, install/update/rollback/remove in isolated staging and boot the verified versions in QEMU; no on-device installer yet |
| R031 | P2 | Security model, authentication, updater, recovery installer | Isolation review, controlled permission tests, signed rollback and virtual-disk install |
| R032 | P2 | Physical PC/ARM64 hardware-portability program | Published test matrix, qualified devices, measured boot/install performance |
| R033 | P2 | Policy-gated AI app assistant | AI chooses catalog item but package verifier enforces approval, signatures, compatibility and rollback |
| R034+ | P3 | Explicit Linux/Windows/Android and lawful macOS routes | Per-app ABI/runtime acceptance on tested configurations, no universal guarantee |

R022 and R023 are **kernel-library foundations**, not proof that filesystem
syscalls or scheduler lifecycle notifications are wired into live ring-3
execution. Those integrations belong to R025 and require new QEMU tests.

## IndianOS original design system and three-edition UX backlog (added 2026-10-28)

**Status: all tasks below are planned, not implemented or approved.** Product
direction: **one independent IndianOS kernel, three distinct experiences**:
**Home Canvas** (Desktop), **Workgrid** (Developer Workstation), and **Ops
Plane** (headless-first Server). Share secure OS services and reusable design
tokens, but do not clone the distinctive layout, artwork, icons, logos,
animations or proprietary UI assets of Windows, macOS, Linux distributions or
other products. Familiar accessibility conventions are permitted. No
unqualified originality, legal clearance, or hardware/app compatibility claim
may be made without review and evidence.

Proposed v0.1 design tokens (subject to accessibility and brand review):
main ink `#132337`, teal `#007D77`, amber `#C88726`, slate
`#52677B`, mist `#F6F8F7`, dark background `#0A1321`, error
`#B63B4B`. The proposed logo exploration is **Three Paths, One Core**,
symbolizing the three editions and shared kernel; it is a *concept*, not a
released logo or cleared trademark. Plan English/Hindi and Devanagari text
shaping with properly licensed fonts.

| ID | Priority | Deliverable | Acceptance / evidence required |
|---|---|---|---|
| UX01 | P0 | IndianOS brand & originality specification | Record logo sketches, unique identity rationale, brand palette and naming rules; document provenance and visual-similarity/trademark review **before** public logo release; no copied third-party assets |
| UX02 | P0 | Shared design tokens, typography, original icon library | Reusable light/dark colors, spacing, component states, type scales, keyboard-focus styles and custom icons; contrast/accessibility checks and verified font licenses including Devanagari |
| UX03 | P0 | User-space compositor & safe surface protocol foundation | Complete pending R029 ring-3 window server, per-app surface isolation, event-driven input and focus; QEMU pixel/input tests and no untrusted app access to another app's memory |
| UX04 | P1 | Shared component library and shell interaction patterns | Working buttons, menus, dialogs, notifications, files/permissions prompts and error/loading/empty states; keyboard navigation, reduced motion and regression tests |
| UX05 | P1 | Desktop / **Home Canvas** edition | Functional home, application launcher, file manager, search, settings, notifications and update/recovery screens; real working navigation and QEMU screenshot + interaction evidence rather than static mocks |
| UX06 | P1 | Developer Workstation / **Workgrid** edition | Resizable project workspaces, terminal, Git/build/test feedback and project session restore; integrate service/database/debugging panels only when underlying backends exist; automated workflow tests |
| UX07 | P1 | Server / **Ops Plane** edition | Headless boot with optional authenticated remote admin: services, deployments, metrics, logs, network, security, backups and recovery; prove server operation without a graphical UI and permission/audit tests |
| UX08 | P1 | Unified, hardware-aware installer and edition selection | Detect supported CPU/firmware/storage, explain device/partition impact, select Desktop/Developer/Server profile, refuse unsupported devices and destructive defaults; disposable-QEMU-disk install/rollback tests before real hardware |
| UX09 | P1 | Shared edition profiles and service/package manifests | One kernel, separate default services/UI; Server excludes heavyweight desktop by default; switching/add-on features uses validated packages and never bypasses permission checks |
| UX10 | P1 | Unified files and app-install experience | VFS file-picker and permissions design, tested filesystem drivers, truthful supported-format checks, signed native app installation; do not imply that `.exe`, `.app`, `.deb` or `.apk` runs merely from its extension |
| UX11 | P1 | Accessibility, multilingual and responsive UX validation | English/Hindi flows, Indic input/text shaping, keyboard-only operation, screen-reader hooks, scaling, contrast, reduced motion, error guidance; tested across all three editions |
| UX12 | P1 | Updates, security and recovery UX | Clear staged/verified updates, user-visible approval for disruptive changes, signed packages, rollback and recovery flows; test interrupted update and failed signature behavior |
| UX13 | P2 | Original motion, wallpaper, boot and lock-screen identity | Distinctive but restrained animations and legally original assets; performance budgets, reduced-motion alternatives and pixel tests; do not hold up kernel reliability work |
| UX14 | P2 | Three-edition end-to-end visual acceptance & release criteria | Independent actual QEMU screenshots/video captures and functional smoke tests for every screen; no mockups claimed as implemented; publish verified supported hardware/feature matrix |

**Implementation order / dependency gates:**

1. Lock `UX01–UX02` specification only after originality, licensing, and accessibility review. Draft visuals are provisional.
2. Finish `UX03` (R029 compositor/security foundation) before claiming production Desktop UI; develop `UX04` and early Home Canvas alongside bounded QEMU acceptance.
3. Deliver working `UX05` Desktop screens, then reusable `UX06` developer workflows; neither edition may claim Linux/Windows/macOS application compatibility until it is verified app-by-app.
4. Deliver `UX07` Server headless services and authenticated optional Ops Plane. Use shared kernel/services, **not** three forked kernels.
5. Deliver `UX08–UX12` installer/profile, filesystem/app integration, accessibility, signed updates and recovery with safety gates; `UX13–UX14` polish and release evidence follow functional acceptance.

**Definition of done:** a design mockup or passing unit test is insufficient:
each displayed feature must have a real event-to-visible-result path, documented
fallback/error handling, applicable security checks and reproducible QEMU or
hardware evidence. Preserve existing kernel, boot and CI regressions.

## Commit / validation policy

1. Work on an isolated verification branch to run host regression tests,
   `cargo build --no-default-features --target x86_64-unknown-none`,
   source audit, and targeted safety tests.
2. Commit only verified bounded source to `main`, fast-forward only, after
   checking the branch lease. Do not force-push or bypass validation.
3. Run full mandatory QEMU kernel CI on the final `main` SHA. Document CI
   separately from the branch test results; do not declare complete until it
   passes.
4. Never auto-format physical drives or enable experimental IRQs in ordinary
   boots. Preserve tested fallbacks and crash diagnostics.
5. Prioritize working integrations and observed user-visible behavior over
   creating isolated modules merely to increase commit counts.

A 100% Windows/Linux/macOS/Android binary-compatibility target is not
technically guaranteed; support must be measured app-by-app.

## R024 acceptance — Verified 2026-10-23

The R024 xAPIC verification job passed QEMU/OVMF and observed
`APIC_TIMER_PREEMPTED` for both CPU-bound ring-3 PIDs before either process
exited, scheduler resumption of both PIDs, eventual successful exits and
memory-frame reaping. Hardware APIC vector 0x40, supervisor UC MMIO,
PIT channel 2 timer calibration, and the spurious-vector handler are
isolated behind the opt-in preemption test. Legacy PIC IRQ0 no longer supplies
R024 scheduling. The main full kernel CI now treats the QEMU preemption
step as required, not a non-blocking experiment.

This verified test is specific to x86-64 QEMU/OVMF; SMP, real hardware,
secure interrupt-controller initialization for diverse firmware and
production-grade scheduler fairness remain unqualified.

## R025.1 — QEMU-verified secure IPC (2026-10-23)

The [R025 verification run](https://github.com/pramod07info/indianos/actions/runs/39916988928)
passed host policy tests, freestanding Rust compilation, source auditing,
and a real two-process QEMU ring-3 ping/reply. The sender ID came from
the active PCB and matching scheduler task. It rejected invalid recipient
PIDs and user pointers, preserved user output on empty queues, removed
mailboxes at process exit, and reaped both processes exactly once.

R025 **is not complete**: capability-mediated authorization, secure file
syscalls, process-service APIs and general blocking IPC still require
implementation and QEMU regression testing. The ordinary kernel boot
continues to keep this test-only IPC ABI disabled. R025.1 is now required
in full kernel CI, not merely a one-off branch verification.

## R025 completion acceptance (2026-10-23)

The end-to-end `test-r025-complete-qemu` verifies two independent
ring-3 processes, kernel-derived identities, constrained capability
grants, IPC message permissions and responses, process status queries,
per-process TinyFs namespaces, file create/read/delete syscalls,
foreign-object rejection, safe bounded user buffers, explicit token
revocation, and automatic capabilities/mailbox cleanup during process
exit. R025.1's original non-capability test continues to run separately.

The R025 full QEMU execution is mandatory in the kernel CI workflow.
Only the bounded experimental QEMU service is complete: multiuser,
persistent and crash-recoverable file services, blocking multiprocess
IPC, SMP and real-hardware security are outside this acceptance.

## R026 acceptance — Live interactive init (2026-10-23)

The [R026 QEMU verification](https://github.com/pramod07info/indianos/actions/runs/39924988234)
passed kernel process/syscall tests, Rust command-parser regressions,
freestanding no_std compilation, source audit and live QEMU serial scripting.
The shell remained alive through the exact sequence `help`, `ps`,
`run /bin/worker`, `ps`, `wait /bin/worker`, duplicate `run`,
and `exit`. The worker was not runnable before the authenticated launch.
After the command, the worker ran in its distinct ring-3 image, exited
and was reaped before PID1 resumed and accepted more commands.
The shell confirmed the child's state, rejected relaunch and shut down
only at the user's request. Both PIDs were reaped exactly once.

The ordinary default kernel does **not** start this experimental shell.
The service supports one allowlisted worker launch per boot; arbitrary
ELF execution, repeated process creation, persistent filesystem, job
control, background services and multiuser sessions remain outside
R026's *bounded QEMU* acceptance and must be developed later.

Full `main` kernel CI requires the R026 serial-interactive QEMU test.

## R027 acceptance — Persistent QEMU disk and metadata recovery (2026-10-23)

The [R027 QEMU verification run](https://github.com/pramod07info/indianos/actions/runs/39929748026)
passed Rust power-failure tests, freestanding compilation, the prior TinyFs
ATA regression and a five-boot test on the **same temporary 128-sector disk**.
The emulator identified its primary-master IDE disk through ATA IDENTIFY;
the kernel used a bounded volume inside it, committed a multi-sector file,
rebooted to read it, overwrote it, rebooted to read the newer data, then
recovered after the host deliberately damaged the newest metadata copy.
Another reboot verified the recovered data. All sectors outside the selected
volume remained untouched.

The new R027 format is distinct from legacy R016 TinyFs. It has eight
fixed-size root-directory file slots, up to 2048 bytes per file, a pair of
checksummed superblocks and three copy-on-write banks per slot. ATA writes
are flushed before metadata publication. The formatter rejects a nonblank
selected volume. Normal kernel boots never run the experimental disk test.

**Boundaries:** This provides no generalized PCI device enumeration,
virtio-blk, AHCI, NVMe, partitions/GPT parsing, free-space allocator,
arbitrary nested directories, production-grade POSIX VFS, cross-process
locking, encryption, or verified physical power-loss guarantees. The
separate ATA driver only discovers one legacy IDE primary-master in QEMU;
physical disk writes and default autoformat remain forbidden.

## R028 acceptance — Real QEMU Ethernet packets (2026-10-23)

The [passing R028 verification run](https://github.com/pramod07info/indianos/actions/runs/39933935521)
built the freestanding kernel, passed **71 Rust unit tests**, audited source
ownership and booted QEMU/OVMF with a real emulated legacy virtio-net PCI
device. On the isolated host-side QEMU socket network backend, the kernel
performed strict Ethernet ARP resolution, IPv4 ICMP ping, a UDP
request/response, an A-record DNS query for `indianos.test`, and one TCP
SYN/SYN-ACK/ACK handshake with validated request/response payloads.
The host verified every required Ethernet frame (including checksum,
address, port, length and sequence checks); the guest emitted ordered
success markers. OVMF's unsolicited pre-boot IPv6 neighbor solicitation
is ignored only while the R028 protocol demo has not begun.

The driver uses two bounded physically contiguous legacy virtqueues, PCI
I/O bar discovery, DMA bounce buffers, and one-CPU polling. Networking is
**off in ordinary boot** and uses an isolated loopback socket peer rather
than public internet connectivity. TCP is a single stop-and-wait
demonstration: DHCP, configurable IP, generic TCP sockets, retransmission,
multiple flows, inbound listening, fragmented IP, network security,
firewall, modern virtio, SMP and real-NIC hardware are not implemented.
The full `main` CI requires the R028 real QEMU networking test.

## R029 visual acceptance (2026-10-23)

The [R029 live QEMU verification](https://github.com/pramod07info/indianos/actions/runs/39941297124)
passed host Rust ABI linkage, GUI model/bounds tests, freestanding Rust
compilation and a **real 1280×800 QEMU VGA display**, screenshot-verified
through the QEMU monitor's `screendump` command. The opt-in compositor paints
two distinct window bodies/titlebars over a software-rendered desktop and
receives an actual PS/2 auxiliary mouse click selecting Files. The test then
sends seven COM1 keyboard actions, observing changed framebuffer pixels on
each focus, move, minimize, restore and close; the quit command exits with
one remaining window and correct input-event accounting.

The current compositor runs in the **trusted ring-0 kernel** under a dedicated
feature, not in an unprivileged ring-3 process. The original R029 intention
of a user-space compositor and message-based application surface isolation
remains outstanding; this graphical achievement is deliberately **not**
proof of general desktop readiness. It is disabled in ordinary boot.

The full main kernel CI requires the QEMU framebuffer/mouse acceptance test.

## R029 ring-3 surface commands — QEMU acceptance (2026-10-23)

The [passing verification run #39949896007](https://github.com/pramod07info/indianos/actions/runs/39949896007)
passed **117** kernel Rust unit tests, the C-ABI host regression, a freestanding
Rust kernel compile, source audit, and real 1280×800 QEMU VGA screenshot
comparisons. An independently mapped ring-3 ELF performed a host-stepped
userland draw of Files and Terminal content via syscalls; framebuffer mapping
was never delegated to user pages. The kernel authenticated PID1,
copied and validated each full descriptor, rejected forged pointers and
out-of-bounds rectangles and kept chrome unchanged. Original ring-3 runtime
reaped the process and restored all allocated frames. QEMU screenshots
confirmed 229, 279 and 1,532 sampled pixel differences at the three
render/focus phases.

The **full kernel compositor still runs in ring 0** and the single
trusted PID1 drawing client controls both sample windows. A complete
ring-3 window server with independent application surface ownership
and event delivery is not yet implemented; this is a bounded,
verified transition toward that architecture.

## R030 verified signed app SDK (2026-10-23)

[Full R030 verification #39959376254](https://github.com/pramod07info/indianos/actions/runs/39959376254)
passed six Ed25519 crypto and package lifecycle tests, the existing kernel
host ABI regression and a live QEMU chain that booted signed native ELF v1,
then v2 following a verified update, then v1 after rollback. The host
rejected a damaged archive before modifying the installed application and
verified remove. This is a **developer-host SDK**: the live guest's native
on-device package database/installer, persistent app store and dynamic
runtime launcher remain outside the present implementation.
