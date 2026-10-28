# IndianOS edition profiles and All-in-One installation

**Status:** Proposed product and engineering specification (2026-10-28).
Not implemented; no supported physical installer or completed Server/
Developer edition is claimed.

## Product decision

IndianOS ships **one independent Rust-first kernel** and optional user-space
packages. Users can install one or more of three editions through **four
presets**:

| Installer preset | Installs by default | Default boot and service policy |
|---|---|---|
| Desktop — Home Canvas | Core + desktop shell + everyday tools | GUI starts; server workloads disabled unless explicitly enabled |
| Developer Workstation — Workgrid | Core + desktop shell + opt-in developer tooling | GUI starts; IDE/database/VM workloads on-demand, not all at boot |
| Server — Ops Plane | Core + service supervision + network/security + optional admin panel | **Headless** by default; authorized selected server services may autostart |
| All-in-One | Union of Desktop + Developer + Server profile packages | GUI starts; development tools and most server workloads remain stopped/on-demand, unless explicitly enabled |

**Custom install:** select any nonempty subset of Desktop, Developer and
Server (including all three). All-in-One is a shortcut for all three, not an
extra OS or extra kernel. Users can change the selection later where an
installer/signed-package capability and hardware support have been verified.

## Proposed user installation workflow

1. Verify CPU/firmware/device compatibility, boot trust and available disk
   space. If unsupported, stop with actionable diagnostics.
2. Detect existing partitions and filesystems **read-only**; show backup
   reminder and whether any requested action erases or modifies data.
3. Present Desktop / Developer / Server / All-in-One, plus a custom multiselect
   profile option. Show package space estimates separately from running
   workload RAM needs; never disguise them as verified measurements.
4. Resolve manifest requirements/conflicts, versioned ABI compatibility,
   signed packages and hardware-dependent drivers. Present conflicts before
   the user authorizes any write.
5. Show final storage, boot mode, services that will start automatically and
   optional features that remain disabled. Explicit consent for destructive
   partition operations; no automatic format.
6. Execute verified staging and atomic/rollback-safe operations on **disposable
   QEMU disks** first. Power-failure or failed signature must recover to the
   previous bootable profile, preserving user files.
7. On first boot start kernel + mandatory security services + chosen
   supervisor; enable Desktop GUI only when selected, and start server
   services only according to explicit authorized policy.
8. Provide post-install **Features & Services** management for installing,
   enabling, disabling and updating profiles without reinstallation.
   Require elevated authorization for security-sensitive changes.

## Package and service manifest contract (concept)

Each edition is a signed, versioned, declarative manifest:

- `profile_id`, `manifest_version`, supported `architecture` and
  `kernel_abi_min/max`
- `required_packages`, `optional_packages`, dependencies and conflicts
- `services`: `required`, `on_demand`, `disabled`, or
  `user_approved_always_on`, with startup, shutdown and health-check policies
- Explicit capabilities, file/network rights, user groups and secrets policy
- Resource reservations/limits and versioned configuration migration rules
- Signature provenance, update/rollback policy and recovery fallback

Manifest composition uses a deterministic validated union with a shared
core and deduplicated packages. Profile overrides must not weaken mandatory
kernel/security controls. A signed manifest is insufficient by itself: the
installer must also verify policy, hardware/ABI fit and integrity.

## Lightweight-by-default resource strategy

- No separate kernels; install one shared runtime and reusable libraries.
- Installed != running: optional IDEs, databases, containers, VMs and AI
  runtimes must not consume idle CPU/RAM merely because selected in the
  All-in-One preset.
- Desktop UI stays optional; headless Server skips compositor and graphical
  services. Developer installs tools but launches heavy processes on demand.
- Explicitly enabled servers may continue in background, including during
  desktop sessions, with an understandable stop/start/status interface.
- Proposed kernel/userspace enforcement: scheduler CPU budgets, memory
  accounting/quotas, bounded process/service counts, I/O and socket quotas,
  isolation and priority for responsive input and mandatory kernel services.
  These are **design targets**, not presently proven implementations.
- Track baseline install size, idle/resident RAM, boot time, CPU idle use,
  workload memory and service latency for all four presets on recorded
  identical QEMU configurations. Publish test numbers before committing
  performance or minimum-hardware claims.

## Non-destructive switching examples

- **Desktop -> Desktop + Developer:** install signed developer packages,
  preserve data and user settings, enable terminal/project services, do not
  start every IDE/database.
- **Developer -> All-in-One:** add Server profile, show service ports and
  privileges, obtain permission before enabling always-on listeners.
- **All-in-One -> Desktop:** stop/deactivate optional services safely, check
  dependent applications and preserve user work. Ask before removing packages
  or data; no disk repartition.
- **Headless Server -> GUI-enabled Server:** install optional desktop components
  only after compatibility and memory/disk budget checks; recovery stays
  possible if GUI fails.

## Acceptance and test matrix

- [ ] All four presets and each supported custom combination pass manifest
      resolution and QEMU staged install; expected package union has no
      duplicate core runtimes and no hidden kernel variant.
- [ ] Installer previews storage changes and refuses silent partition
      formatting; corrupted images, low space and failed signatures do not
      corrupt existing user data.
- [ ] Desktop boots GUI; Developer starts a project/terminal; Server boots
      without framebuffer/compositor and exposes only authorized services.
- [ ] All-in-One starts GUI and **does not** automatically launch optional
      IDEs, databases, VMs, AI services or development containers.
- [ ] Explicitly enabled background server survives desktop logout/restart
      per documented policy; stop/disabling revokes its permissions and
      prevents accidental public network exposure.
- [ ] Switching profile selections preserves user data; power-loss and
      interrupted package installation recover/rollback; disable operations
      check dependents.
- [ ] Resource accounting/limits and overload tests show responsive
      controls, bounded background services and clean failure/recovery.
- [ ] Per-preset idle CPU/RAM, storage and cold-boot metrics measured on
      documented QEMU hardware; results are labeled measured vs estimated.
- [ ] Security, filesystem recovery and main kernel CI continue to pass at
      the exact final commit SHA.

## Engineering sequence and backlog ownership

K01–K07 kernel ABI/security design review first; then stable PID1/service
supervisor and on-device signed package manager; then UX08–UX09 manifest
and installer integration; then UX15–UX18 All-in-One selection, profile
transitions, demand-driven service policies, quotas and validation.
Respect authoritative M1–M5 exit conditions. Keep design work parallel
where it does not compromise kernel safety or required tests.

**Current implementation:** planning only; no installed All-in-One
runtime, working profile switcher or production resource quota system is
asserted by this document.
