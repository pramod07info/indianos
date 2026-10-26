# R039 Batch 5 — tasks 2–25: integration-first development

Branch: `batch-5`. One coordinated feature commit, not 24 merge commits.
Baseline: Batch 4 merged `main` at
`3ce88100923ee9cc525aca48f0c65a57ce406230`.

**Scope:** This batch begins implementation across all 24 user-prioritized
tasks. Its executable deliverable is a bounded, no-allocator, host-testable
integration layer that connects existing IndianOS modules, not 24 production
features. The table deliberately identifies missing target-level work; it
would be inaccurate to call a general desktop, physical AHCI/NVMe, internet
TLS, Android/Windows runtime or physical installer complete.

## Implementation and remaining proof

| # | Goal | Implementation slice in this commit | Remaining target-level exit evidence |
|---|---|---|---|
| 2 | Normal desktop boot | `batch5_session` authenticated launch + ordered service startup API | wire real boot PID1/startup path and default QEMU desktop proof |
| 3 | Ring-3 compositor | session-owned windows and focused keyboard routing with epoch teardown | move full compositor/event broker to ring 3 and screenshot attestations |
| 4 | Scheduler/service IPC | service planner, verified launch intents and epoch-authenticated event delivery | real scheduler/syscall ingress, timers, crash restart QEMU |
| 5 | Persistent hierarchical VFS | `batch5_vfs` canonical paths + PID-bound file descriptors + backend read adapter | read/write/fsync/rename from guest durable hierarchical VFS |
| 6 | On-device signed installer | `batch5_installer` revalidates signature against exact payload at commit | real Ed25519/SHA-256 verified on guest and transactional disk |
| 7 | Auth/capabilities | generation-scoped sessions, file handles, window/keyboard IPC and consent | authenticated user sessions and untrusted ring-3 syscall test |
| 8 | A/B updates/rollback | install adapter binds existing rollback journal and health gate | durable bootloader slot switching, power-cut and recovery |
| 9 | Files/Terminal/Settings | session connects window/input and per-user settings; VFS read adapter | real app rendering and input actions in normal boot |
| 10 | End-to-end QA | `batch5_acceptance` integrated host regression | real desktop/input/install/recovery QEMU acceptance tests |
| 11 | PCI/ACPI initialization | `batch5_devices` ACPI MADT header acceptance and authenticated MMIO grant | full firmware discovery, APIC routing and PCI enumeration |
| 12 | AHCI/NVMe/VirtIO | grant-bound AHCI DMA preflight; retained NVMe/VirtIO validators | real PCI MMIO, AHCI HBA, NVMe queues, modern VirtIO and disk I/O |
| 13 | Driver isolation | epoch-checked MMIO permissions and quarantine | actual IOMMU + domain isolation and IRQ dispatch |
| 14 | DHCP/TCP/TLS | `batch5_network` validates BOOTP XID/MAC and DHCP ACK before lease; TLS gate delegates verifier | full network sockets, packet authentication, TLS engine, firewall |
| 15 | AI app installer | signed payload + explicit owner/digest consent and deterministic application commit policy | catalog integration, local LLM, user confirmation UI and secure installer |
| 16 | Linux ELF/POSIX | `batch5_compat` identifies Linux ELF and returns NeedsLinuxRuntime | Linux syscall translation, libc, mmap/signal and app regression |
| 17 | Windows PE/Win32 | PE validator distinguishes Win32 runtime requirement | PE loader, Win32 APIs and licensed dependencies |
| 18 | Android | APK ZIP validation selects Android runtime requirement | ART, Android graphics/input/runtime and permissions |
| 19 | macOS legal routes | portability, web or authorized remote Mac route | tested per-app native port or licensed remote Mac integration |
| 20 | Physical PCs | `batch5_devices` safe device lease model and `batch5_release` platform-specific evidence | actual tested hardware, firmware, GPU/input/storage drivers |
| 21 | Crash recovery | `batch5_release` trusted fault logging adapter | persistent crash dumps and system watchdog |
| 22 | Security audit | independent-review release gate and malformed/unauthorized input tests | actual external audit, memory safety attack-surface assessment |
| 23 | Performance | measured tick-budget release gate | physical boot, memory, throughput profiling and tuning |
| 24 | Safe OS installer | verified app install backend contract and rollback primitives | physical disk partitioning, bootloader, safe UI/recovery |
| 25 | Release qualification | exact-SHA QEMU vs hardware evidence and independent-review gate | hardware matrix, accessibility, Hindi localization, actual release QA |

## Safety and test criteria

- Never trust a model-selected application as signed.
- Stage only an authenticated package image, check signature and signed
  payload **again** before any backend calls, and demand explicit consent
  from the original owner generation/digest.
- `PackageStore`, `GuestVfs`, driver discovery, crypto and network
  verification backends are **external contracts**. Their stub tests do not
  prove production durability or encryption.
- A retired process cannot continue receiving events, keyboard input, files
  or window ownership; a reused PID must carry a new generation.
- If CI is missing/skipped/failed on this exact SHA, do not merge.
- Test command: `cargo test --offline --manifest-path rust/kernel_core/Cargo.toml`
  and full mandatory GitHub Actions `IndianOS kernel boot (x86-64)`.
- Need a follow-on QEMU milestone to connect the session and installer
  APIs to actual ring-3 code; do not advertise the desktop as complete.

## Merge sequence

1. Verify post-Batch-4 `main` kernel, filesystem and SHA checks.
2. Confirm Batch 5 remains **one** commit on `batch-5`.
3. Require fresh, exact Batch 5 SHA full-kernel/QEMU pass and independent
   source review; fix failures in the branch and retain a single squashed
   feature commit where possible.
4. Merge only after gates pass; run the new `main` build before claiming
   combined release readiness.

This batch is intentionally not a substitute for the multi-year,
hardware-dependent production operating-system roadmap.

## India-first accessibility slice

`batch5_accessibility.rs` adds first English/Hindi labels for Desktop,
Files, Terminal, Settings and consent actions. Controls require readable
labels, keyboard activation and a minimum 36×36 logical hit target.
This is locale data and testable UI policy only: Devanagari font rendering,
screen readers, contrast tests and actual GUI wiring still require work.
