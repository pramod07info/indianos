# R040 Batch 6 — one-commit continuation of tasks 2–25

**Repository:** `pramod07info/indianos`
**Branch:** `batch-6`
**Starting point:** merged R039 Batch 5 main SHA
`50cd700155097e08b3d7424216551b14ffb0d125`

## What this single commit actually implements

A second integration layer that **uses** the R039 kernel-policy APIs
rather than creating twenty-four unrelated placeholder stubs.

- `batch6_boot.rs`: dependency-checked init/desktop activation;
  verified image stages, lifecycle phase and crash degradation.
- `batch6_surface.rs`: PID+generation-authenticated surface damage queue,
  local-rectangle bounding, reclamation on process exit, and serials.
- `batch6_security.rs`: finite rights leases for read/write, UI,
  install, driver and network, with expiration and PID reuse denial.
- `batch6_storage.rs`: canonicalized path and authorization for
  all-or-nothing backend write/read with no partial destination mutation.
- `batch6_network.rs`: authenticated per-process expiring port grants
  and fail-closed TLS certificate-verifier dispatch.
- `batch6_recovery.rs`: bounded crash/restart policy and measured boot gates.
- `batch6_devices.rs`: PCI storage class preflight with driver MMIO lease
  and quarantine; no hardware initialization or DMA.
- `batch6_apps.rs`: trusted app selection, signed-content format
  classification, and explicit `RuntimeMissing` for Linux/Windows/Android
  programs, plus lawful macOS alternatives.
- `batch6_release.rs`: exact-Git-SHA QEMU/physical-platform,
  screenshot/input/recovery and independent-review evidence gate.
- `batch6_acceptance.rs`: combined desktop/IPC/driver/filesystem/compatibility
  acceptance (host-only); focused tests in each module.

## Tasks 2–25: work completed in this commit vs still outstanding

| Task | Implemented in R040 | Still required for production completion |
|---|---|---|
| 2 Normal desktop boot | Authenticated init-before-desktop boot-plan transaction | Boot PID1 and ring-3 desktop by default; direct QEMU proof |
| 3 Ring-3 compositor | Epoch-authenticated, bounded surface-damage broker | Full compositor in ring-3 and actual framebuffer update path |
| 4 Scheduler/service IPC | Restart-aware service lifecycle and identity teardown | Connect scheduling, interrupts and real syscall ingress |
| 5 Persistent hierarchical filesystem | Authorization-checked atomic guest VFS backend contract | Durable guest write, fsync, directory rename, crash/power-loss QEMU |
| 6 Signed app installation | Strengthened access boundary; uses R039 signed-image installer | Real on-device Ed25519/hash verifier, package store, loader |
| 7 Authentication/permissions | PID/epoch/TTL rights leasing and revocation | Real login sessions, untrusted user-mode capability validation |
| 8 A/B updates | Durable-update backend and finite restart policy contract | Bootloader A/B, signed update payload and power-loss rollback tests |
| 9 Desktop apps | Window-damage broker and verified Files/Settings session state | Working default Files, Terminal, Settings and Installer UI |
| 10 End-to-end QA | One integrated Rust acceptance suite | Real GUI+disk+network+crash QEMU screenshot and restart suite |
| 11 PCI and ACPI | PCI storage header classifications with resource lease | Live PCI traversal, ACPI MADT, interrupt controller routing |
| 12 Storage drivers | AHCI/NVMe/VirtIO policy enumeration and MMIO validation | Working AHCI/NVMe queues, DMA and modern VirtIO hardware I/O |
| 13 Driver isolation | Kernel-granted, generation-authenticated driver MMIO lease | IOMMU + interrupt routing + driver user process fault containment |
| 14 Networking | PID-bound expiring port leases, TLS peer-verifier dispatch | Real socket API, TCP reliability, TLS implementation, routing/firewall |
| 15 AI app assistant | Deterministic signed catalog and compatibility route gate | Local language model, visible approval and live signed app install |
| 16 Linux compatibility | Detect ELF64 as requiring Linux runtime | POSIX, ELF loader, libc, signals and test apps |
| 17 Windows compatibility | Detect PE as requiring Win32 runtime | PE loader and usable licensed Win32 API coverage |
| 18 Android compatibility | APK classification as requiring Android runtime | ART/Android runtime and graphics/input app support |
| 19 macOS routes | Portable/web/authorized-remote paths remain lawful | Port and verify real apps or licensed remote Mac connector |
| 20 Physical PC support | Trusted PCI candidate classification, per-platform evidence | Physical device drivers and diverse hardware test matrix |
| 21 Crash and recovery | Bounded crash sequence and restart allowance | Persistent crashlog, watchdog and recovery UI |
| 22 Independent security audit | Expiring-rights adversarial tests and review evidence gates | Third-party audit, fuzzing, threat model and penetration test |
| 23 Performance optimization | Measured boot budget with no fabricated samples | Optimize real boot, I/O, RAM, responsiveness on QEMU/PC |
| 24 Safe OS installer | Atomic backend contract and rollback policy hooks | UEFI disk installer and recovery, partition safety on physical PCs |
| 25 Public release | Exact SHA/review/platform/interactive evidence gate | Accessibility testing, Devanagari fonts, Hindi UI, QA/support |

All 24 items have a **bounded, testable development slice**, not 24
complete products. A single Git commit is a packaging requirement; it is
not evidence of OS release readiness or hardware compatibility.

## Required CI gate and merge workflow

1. R039 Batch 5 post-merge main kernel, filesystem and SHA validation
   must be successful at the **exact** `50cd7001...` SHA.
2. R040 branch is exactly one feature commit, with no direct main writes.
3. Run `cargo test --offline --manifest-path rust/kernel_core/Cargo.toml`
   and freestanding kernel/no_std cross build through full GitHub
   `IndianOS kernel boot (x86-64)` QEMU Actions workflow.
4. Check `Roadmap SHA Validation` against the same branch SHA.
5. Fix every test failure; do **not** skip, disable or weaken CI.
6. Obtain independent source/security review, then merge and run a
   fresh exact-main-sha kernel/full build.

**Caveat:** Hardware DMA, real package cryptography, TLS and application
runtimes must never be inferred from Rust policy/unit tests.
