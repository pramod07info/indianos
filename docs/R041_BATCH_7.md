# R041 / Batch 7 — runtime-first development on tasks 2 through 25

**Branch:** `batch-7`. **Exactly one feature commit**, based on merged
Batch 6 main commit `66317947739c584643ddaec440318e65d2ba5e78`.
**Status:** Development branch, not yet a public OS release.

## Actual functional implementation added

The milestone is an executable no-allocator RAM-backed Files/Terminal
workspace. This is a material step beyond Batch 5 and 6's interface-only
policies, but **it is not persistent to disk and is not yet wired to an
actual default-boot ring-3 process**.

1. `batch7_ramfs.rs` provides real file storage (capacity-limited,
   hierarchical absolute names, atomic in-process whole-file replacement,
   byte-exact reads, bounded directory listing, monotonically versioned
   writes). Implements both the prior `GuestVfs` and `AtomicFileBackend`
   traits. Rejections do not mutate the previous file. Reboot loses RAM.
2. `batch7_terminal.rs` implements a safe interactive command buffer,
   backspace, submit, and working `help`, `pwd`, `ls`, `cat /path`,
   `write /path data`. Real file bytes are returned, with each read/write
   authenticated against existing PID-generation rights and deadline.
   There is **no arbitrary command execution**.
3. `batch7_workspace.rs` composes the verified launch service
   transaction, owned desktop window, permission leases, live Terminal
   commands, RAM files and per-owner damage events. Process exit revokes
   UI/permissions, retains only in-memory file contents and rejects stale
   PIDs.
4. `batch7_acceptance.rs` exercises keyboard-like typing, actual file
   writes and reads, listing, UI damage, retirement, expiration and
   compatibility/signed-package denials.
5. Add Cargo `test-r041-workspace` **opt-in** feature and QEMU kernel-main
   smoke call. It checks the Rust workspace read/write/retire integration
   in a freestanding kernel build and logs an explicit R041 marker.
   It is not a real ring-3 compositor or a fully interactive GUI.

## Task-by-task work and remaining acceptance (2–25)

| Task | R041 work or retained integration | Remaining to consider complete |
|---|---|---|
| 2 Boot interactive desktop | Workspace/service startup transaction, opt-in no_std QEMU smoke | Normal boot ring-3 PID1 and desktop service, actual screenshot |
| 3 Ring-3 compositor | Owner-bound window damage/event queue in working workspace | Real userspace compositing, owned pixel surfaces and input broker |
| 4 Scheduler/services | Verified init-before-desktop and epoch teardown | Real process loader, syscalls, IRQs and restart supervisor in QEMU |
| 5 Persistent filesystem | Working bounded RAMFs supporting read/write and VFS descriptors | Replace RAM backend with durable hierarchical on-disk VFS, fsync/reboot |
| 6 Signed package installer | Retains Batch 5 signed-image/consent/rollback policy; tests fail-closed trust | Real on-device Ed25519 verification, package DB, install/run in guest |
| 7 User auth & permissions | Workspace checks actual PID/epoch/expiry on commands | Real user sign-in and untrusted ring-3 syscall API |
| 8 A/B updates | In-memory atomic replacement semantics + retained rollback journal | Durable boot slot, power failure and signed system updates |
| 9 Desktop apps | Actual Terminal `ls/cat/write` and Files data; localized strings | Wire to real graphical interactive apps with normal boot and keyboard |
| 10 End-to-end tests | Host cross-module tests + opt-in freestanding QEMU marker | Direct screenshot, keyboard, filesystem reboot/recovery acceptance |
| 11 PCI/ACPI | Retains Batch 6 safe PCI discovery preflight | Real firmware and PCI enumeration, ACPI interrupt mapping |
| 12 Storage drivers | RAM backend exercises filesystem independent of external disks | Real AHCI/NVMe/modern VirtIO DMA/IRQ I/O |
| 13 Driver isolation | Retains generation-scoped MMIO grant + quarantine | IOMMU, safe mapping and driver process fault isolation |
| 14 Networking | Retains DHCP identity checks, expiring socket/port grants, TLS gate | Reliable TCP/UDP sockets, actual TLS engine, routing, firewall |
| 15 AI installer | Retains deterministic trusted catalog and consent-denial tests | Integrate actual local AI model and signed-app UI flow |
| 16 Linux compatibility | Explicit Linux ELF detection and runtime-missing path tested | POSIX syscalls, memory map, libc and real app execution |
| 17 Windows compatibility | Retains PE validation and runtime-required classification | Win32/NT runtime and genuine tested Windows applications |
| 18 Android compatibility | Retains safe APK ZIP parser and runtime-required classification | ART/runtime, graphics, APK permissions and real execution |
| 19 macOS interoperability | Retains lawful portable/web/authorized remote route policy | Real tested native ports/web/authorized remote service |
| 20 Physical PC support | Retains per-platform evidence + MMIO preflight gates | Real hardware validation, desktop/display/input/storage network |
| 21 Crash diagnostics | Regression tests finite restarts and process revocation | Persistent crash logs, watchdog, safe recover and debugging |
| 22 Security audit | Real rights enforcement and adversarial command/path tests | Independent audit, fuzzing, kernel threat model, attack simulation |
| 23 Performance | Bounded RAM usage and retained measured boot budget | Benchmark measured real boot, I/O, memory/graphics performance |
| 24 Installer/recovery | Atomic file replacement interface retained | Physical disk partition/installer, UEFI entry and rescue environment |
| 25 Release qualification | Exact-SHA host/QEMU gates, localized string and accessibility regression | Full device matrix, accessible desktop testing, fonts, independent review |

## Acceptance and merge conditions

- `cargo test --offline --manifest-path rust/kernel_core/Cargo.toml`
  must pass for the full crate.
- The kernel, QEMU and SHA-validation jobs MUST pass on this **exact**
  Batch 7 commit. Tests should not be skipped/disabled for green status.
- The opt-in `test-r041-workspace` path is code and requires a separately
  recorded guest QEMU run to claim it has actually passed.
- No real disk persistence, physical hardware driver, signed crypto engine,
  userland compositor or Linux/Windows/Android runtime is claimed here.
- Obtain independent review before merge. Full kernel + filesystem tests
  must also pass on the previous Batch 6 merged `main` SHA.
