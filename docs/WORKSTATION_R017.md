# R017 — Developer Workstation / 20-workstream integration

**Baseline:** f76eab1a3dfb06d1a6627585c264a1037026b485

This batch adds tested, bounded workstation primitives. It is one **coordinator source commit and one integration PR**. Agent labels are scoped workstreams; no twenty independent coding processes were launched.

## Workstreams
01 project session / revisioned scripts — `batch7_workspace.rs`
02 terminal line editing — `batch7_terminal.rs`
03 editor save, append and truncate — `batch7_ramfs.rs`
04 durable project-file checksums and comparisons — `hier_vfs.rs`
05 split-screen window tiling and atomic layout changes — `batch2_windows.rs`
06 per-window input acknowledgements and owner privacy — `gui_broker.rs`
07 native toolchain dependency and rights planning — `m2_sdk.rs`
08 app-store inventory snapshots — `m2_store.rs`
09 signed-bundle dependency preflight — `m2_bundle_manifest.rs`
10 service process supervision and inspection — `service_supervisor.rs`
11 installed-app syscall batch sandbox preflight — `m2_syscall_guard.rs`
12 scoped network development sessions — `m2_network.rs`
13 auditable filesystem/network grants — `batch6_security.rs`
14 per-core process inspection and incarnation teardown — `process_events.rs`
15 A/B update decision status and restart consent — `batch3_updates.rs`
16 CPU quantum simulation quota monitoring — `quantum_service.rs`
17 CPU fallback focus/window pixel rendering — `hardware_virtio_gpu.rs`
18 POSIX file descriptor inspection and atomic closure — `batch4_linux_abi.rs`
19 safe native/foreign application execution routing — `batch6_apps.rs`
20 PID-generation-scoped crash diagnostics — `batch3_crashlog.rs`

## One build acceptance
- Rust host unit tests (including the three+ negative cases per workstream).
- `test-r041-workspace` QEMU mode must print `IndianOS: R017 workstation RAM project editing PASSED`.
- Full standard kernel/QEMU boot, security S0 and SHA policy validation.
- No merge until exact-head CI success, independent review, and stable main baseline.

## Scope gaps / truth in milestones
- The IDE/editor is an actual bounded RAM file API, **not** yet a full-featured visual code editor.
- The terminal line editor executes only the already supported built-in commands, **not** arbitrary build processes.
- Host unit and opt-in guest smoke do not establish normal boot Desktop/Developer/Server profile selection, user-space compositor, hardware GPU acceleration, external app compatibility, true TLS, real QPU access or physical-disk durability.
- A/B updater status is policy only; the actual signed downloader, block staging and bootloader must enforce the security contract.
