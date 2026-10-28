# IndianOS master milestones — M1 through M5

**Status:** Authoritative outcome-based IndianOS delivery roadmap.
**Last reconciled with repository:** 2026-10-25.

This is the persistent milestone structure requested for all further
development. Milestone completion means user-visible behavior verified by
QEMU and physical hardware as specified, not merely code merged or host
unit tests passing. Each milestone can have multiple **one-commit batches**;
never confuse a single commit with completing a multi-year milestone.

## M1 — Bootable, usable IndianOS desktop (current)

**Full requirements ledger for M1:** [M1 all required phases and 99 task gates](M1_PHASE_SCOPE.md). No single existing change-set fulfills every phase; this ledger is the explicit completion contract.

Dependency tracks: Boot/kernel, user/kernel isolation, process scheduler,
persistent VFS, essential drivers, service manager, userspace compositor,
Files and Terminal, and integration/QA.

- [x] Independent Rust-first kernel entry and QEMU/UEFI boot foundation.
- [x] QEMU-tested ring-3 process, syscall and IPC foundation.
- [x] QEMU-tested limited automatic three-process graphical session
      (R031.1), currently opt-in.
- [x] QEMU-tested bounded hierarchical durable VFS and GUI Files reading
      persistent data on a disposable ATA test disk (R031.9).
- [x] Batch 7 real RAM-only command interpreter and file store (R041).
- [ ] **M1.1 current slice**: authenticated Terminal commands connected to
      checksummed block-backed HierVfs with real read/write/mkdir/rename,
      UI damage and read-after-remount acceptance (Rust test); then
      prove the same live ring-3 GUI commands on QEMU.
- [ ] **M1 partition work**: defensive MBR/GPT header/entry checksum checks,
      bounds and overlap rejection, selected-partition read/write window,
      actual HierVfs remount tests. Physical disk qualification pending.
- [ ] Production normal boot starts PID1, user compositor, Files and Terminal
      without test-only kernel feature flags.
- [ ] Move full compositor and input broker from ring0 to independent ring3
      services; per-app surfaces and processes.
- [ ] Implement keyboard, pointer and graphical Terminal/Files actions from
      physical/simulated devices, not serial control.
- [ ] Production persistent VFS with power-loss-safe file writes, fsync,
      directory traversal, file locks and user isolation.
- [ ] Independent processes can create/read/edit files through live syscall
      ABI; filesystem data survives real emulator reboot.
- [ ] Essential QEMU and selected real-hardware driver support,
      safe shutdown/restart, crash recovery and negative security tests.

**M1 exit:** Fresh normal boot launches actual graphical ring-3 desktop,
user physically interacts with Files/Terminal; a file is created, edited and
read back after reboot; process isolation, screenshot/keyboard/mouse,
filesystem corruption and recovery acceptance all pass at exact commit SHA.

## M2 — Developer Alpha

**Current M2 integrated workstream:** [M2 developer-alpha engineering
acceptance](M2_DEVELOPER_ALPHA.md). One feature commit for this workstream;
production M2 exit requirements stay open until fully verified.


- [ ] Reliable network sockets, DNS, DHCP, TCP/IP and TLS on supported NICs.
- [ ] Native app ABI, SDK, developer toolchain, examples and debugging.
- [ ] Signed package installer/updater and persistent on-device package DB.
- [ ] Application lifecycle and per-app sandbox with working app-store UI.
- [ ] Test third-party native application builds and safe updates.

**M2 exit:** Networked, supported native apps built by outsiders can be
installed, run, updated and removed without privileged developer workarounds.

## Cross-cutting Security Milestone S0–S4 (mandatory release gate)

**Security scope and actionable task register:** [GitHub Issue #136](https://github.com/pramod07info/indianos/issues/136). No OS can honestly promise "100% hacker/virus free"; only claim concrete defenses verified by reproducible tests, real hardware when relevant, and independent review.

- [ ] **S0 first slice:** protect `FileHandles` writes to OS/boot/recovery/keys/snapshots paths and apply a finite per-service write budget; adversarial tests and a dedicated always-running security CI workflow. This is a restricted syscall entrypoint, not global VFS policy.
- [ ] **S1:** proper user/app filesystem capabilities bound to scheduler PID+generation, and enforce on all create/write/unlink/rename/overwrite/mmap/storage entrypoints (not just handles); W^X, sandboxing, isolation and audit events.
- [ ] **S2:** protected authenticated snapshots, immutable retention, backup/restore tested after simulated ransomware and device failure, with monitoring that minimizes false positives.
- [ ] **S3:** independently rooted secure boot, TPM anti-rollback on actual hardware, signed packages/drivers and supply-chain verification, parser fuzzing, cryptographic agility and hardening of network/USB/storage attack surfaces.
- [ ] **S4:** independently reviewed threat model, security audit/penetration testing, physical-machine qualification, release/patch and vulnerability-response process.
- [ ] GitHub branch protection must explicitly require successful security CI and review. Adding a workflow file does **not** automatically enforce merge protection. Never mark S0–S4 finished merely from successful functional tests.

**Release principle:** security claims are revision- and threat-specific; successful QEMU unit tests are not evidence of universal malware resistance.

## M3 — Secure Hardware Beta

- [ ] Qualified UEFI PC/laptop model matrix with storage, display, input,
      audio, wired and prioritized wireless connectivity.
- [ ] Safe signed OS installation onto user-selected physical storage with
      explicit destructive action consent and recovery media.
- [ ] Secure auth, storage encryption, isolation, exploit mitigations,
      trusted/signed updates, A/B rollback and reliable backups.
- [ ] Security audits, kernel/driver fuzzing, long duration stress tests,
      boot/crash telemetry under user privacy controls.
- [ ] Performance and battery/thermal qualification on physical machines.

**M3 exit:** Supported physical devices reliably install, boot, work
offline/online, update/recover, and pass independent security review.

## M4 — Advanced Application Compatibility and AI

- [ ] Linux ELF/POSIX runtime and verified real Linux apps.
- [ ] Windows PE/Win32 compatibility or legally licensed VM/remote routes;
      app-by-app compatibility matrix.
- [ ] Android hosted runtime and tested APK graphics/input/permission model.
- [ ] macOS native port, web or authorized remote Mac routes, not promises
      of arbitrary macOS binaries on non-Apple hardware.
- [ ] User-approved offline-capable local AI assistant, app discovery,
      signed package installation, diagnostics and safe tools.
- [ ] Prompt injection, data privacy, model quality and resource-limit tests.

**M4 exit:** Selected external application samples and real AI tasks pass
repeatable quality/security tests. No universal compatibility claim.

## M5 — Stable Public Release and Expansion

- [ ] Independent QA and threat-model coverage across all supported areas.
- [ ] Reproducible, signed boot/install/update and tested rollback releases.
- [ ] Quality standards for localization, Hindi/English fonts, accessibility,
      support and crash diagnostics.
- [ ] Documentation, API versioning, open-source license compliance,
      publishing and long-term support/patch process.
- [ ] Gradually expand hardware/ARM64/app-support matrix with evidence.
- [ ] Optional CPU/GPU-based quantum simulators and post-quantum libraries
      after core release requirements are stable; real quantum hardware
      remains a separate capability.

**M5 exit:** A maintainable, signed, supportable public OS release passes
the published hardware, accessibility, reliability and security criteria.

## Cross-edition delivery track (planned, not yet implemented)

Detailed behavior: [edition profiles & All-in-One specification](EDITION_PROFILES_AND_ALL_IN_ONE.md).
**One kernel, three editions, four installer presets:** Desktop, Developer
Workstation, Server and **All-in-One** (combined bundle). Users may also select
individual combinations. Four presets do **not** create four kernels.

- [ ] **M1 dependency:** stable normal boot/PID1/service supervisor and
      filesystem foundations required for persistent profile configuration.
      Do not delay mandatory M1 safety/boot acceptance for installer polish.
- [ ] **M2 work:** versioned signed profile/package manifests, dependency
      resolver, on-demand lifecycle, safe enable/disable of developer tools and
      optional server services; verify non-destructive transitions.
- [ ] **M3 work:** secure hardware-aware installer with all four presets,
      warnings and confirmation before partition changes, tested rollback and
      resource constraints with real hardware evidence.
- [ ] **M3+/M5 validation:** measure idle RAM, storage, startup and background
      workload interference for each preset; headless Server must operate
      without GUI; All-in-One must not start unneeded services/VMs by default.
- [ ] Never call the full combination lightweight without published tests;
      workloads such as IDEs, Java servers, VMs and databases have additional
      RAM/CPU/storage requirements beyond the installed OS baseline.

All-in-One is a **feature-selection and service-policy** concern; the native
IndianOS kernel remains shared. Existing milestone exit conditions are
unchanged.

## M1 development branch policy

1. Work in a new M1 feature branch from the last verified main SHA.
2. One integrated feature commit **per batch**; a multi-year milestone
   may require many such batches.
3. Require exact-SHA Rust/no_std host tests, kernel QEMU, VFS/durable
   disk regression and code review; never disable checks to force green.
4. After merging, require new main branch build and record exact SHA.
5. All physical disk writes are prohibited in ordinary development boots.
   Explicit isolated scratch media only for emulator tests; real hardware
   qualification is later in M3.
6. Document precisely which subsystem is real/verified, experimental,
   mocked or not yet implemented; do not claim the whole OS is finished.
