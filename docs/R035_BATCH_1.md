# R035 Batch 1 — ten bounded kernel and governance integrations

Baseline: `25626297f2a311815bff218036dd5301829f4df7` (R034 main kernel/QEMU CI green).
This is ONE integration commit and ONE full-kernel PR verification. Passing host Rust
tests alone is not evidence of physical hardware behavior or a production OS.

| # | Workstream | Verified source-level slice | Still required for production |
|---|---|---|---|
| 01 | Durable filesystem | `block::read_extent_bytes` reads across validated, discontinuous sectors, preflights boundaries | Wire durable hierarchical VFS file extents to process file descriptors and test repeated guest reboot |
| 02 | Package trust | `verify_launch_material` demands trusted manifest signature callback AND a payload digest callback | Real guest Ed25519 and SHA-256 implementations, signed manifest hash extraction, guest ELF loader |
| 03 | Capability lifecycle | `EpochRegistry::revoke_resource` denies stale PID generations and leaves peer capabilities intact | Connect process teardown/revocation paths to live IPC syscalls and adversarial ring-3 tests |
| 04 | Service supervisor | `ServiceTable::stop_exact` rejects stale process generation and wrong service | General init launching, service dependencies, timer-based restart and QEMU lifecycle |
| 05 | Compositor | `SurfaceLayout` owns, bounds and revokes client window geometry | Move actual window server, rendering and event router from ring 0 to ring 3 |
| 06 | UDP ownership | `EpochUdpEndpoints` denies stale PID generations and reaps old messages; wraps validated R034 ingress | User-mode socket syscalls, scheduler coupling, DHCP/routing and real security boundary |
| 07 | PCI discovery | `discover_storage_headers` preflights trusted config snapshots, filters storage classes with no output mutation on error | Actual bus discovery, safe BAR map and AHCI/NVMe drivers |
| 08 | A/B disk metadata | `commit_boot_attempt` and `recover_journal` persist two CRC-checked generations in an explicit device window and tolerate a torn copy | Signed update payload/anti-rollback, recovery partition, power-cut QEMU test |
| 09 | CI identity | SHA checker and regression tests plus new pull_request SHA workflow; coordinator requires it and exact-head full QEMU CI | Independent review of privileged workflow change and post-merge main validation |
| 10 | App resolver | `plan_dependencies` produces cycle-safe topological order and enforces whole-graph permission/trust policy atomically | Signed catalog, installer transaction and on-device cryptographic verification |

## Required build acceptance

- `cargo test --offline --manifest-path rust/kernel_core/Cargo.toml`
- `cargo build --offline --manifest-path rust/kernel_core/Cargo.toml --no-default-features --target x86_64-unknown-none`
- `python3 -m unittest discover -s tests -p test_verify_ci_sha.py -v`
- `make test-rust-source-audit`
- Full `IndianOS kernel boot (x86-64)` PR workflow including emulator, storage, network, GUI, fault isolation and recovery
- `Roadmap SHA Validation` on the same exact PR SHA
- After independent review and merge, new `main` kernel workflow must pass before beginning Batch 2

## Safety boundaries

This milestone NEVER formats a real device, never auto-installs unsigned code, never
trusts a caller-supplied PID, and never turns off required CI checks. A CRC detects
accidental corruption, not malicious rollback; signing must be implemented separately.
The temporary QEMU virtual disk must be explicitly bounded and disposable.
Do not declare any of these ten full production features completed on the strength
of policy/unit tests alone.
