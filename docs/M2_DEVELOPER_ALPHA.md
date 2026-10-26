# M2 — Developer Alpha: one integrated development commit

**Base:** M1 merged to main as `db96028a66e1e884d8e0dfed347f50a009ea5218`.
**Branch:** `milestone/m2-developer-alpha`. **Policy:** one feature commit
for the integrated M2 workstream. This commit is not proof of complete M2.

## Full M2 task coverage, exact meaning

| M2 required exit task | Concrete existing + new work | Still required for 100% |
| --- | --- | --- |
| Network sockets, DHCP, DNS, TCP/IP and TLS | Existing R028 QEMU virtio-net IPv4/ICMP/UDP/DNS and single-flow TCP packet exchange; `m2_network` adds process-bound socket leases, strict DNS A parser and fail-closed injected certificate-verifier hook. | Generic BSD-like sockets, multi-connection reliable TCP, DHCP integration, encrypted TLS record engine, real NIC drivers, actual real-internet usage. |
| Native ABI, developer toolchain, examples and debugging | Existing R030 signed native static-ELF SDK + ring-3 QEMU sample execution; `m2_sdk` declares catalog ABI v1 and topologically sorts dependencies without allocation. | Stabilized ABI/toolchain, third-party docs/versioning, debugger, dynamically loaded libraries, public IDE integration. |
| Signed installer/updater + persistent on-device package DB | Existing OpenSSL Ed25519 host SDK verifies signed archives; limited R030.2 QEMU guest allowlisted metadata install/rollback; `m2_inventory` persists per-app version/rights/digest in checked COW HierVfs, verifies an independent trusted `Verify` implementation at request time, blocks stale identities/downgrades, supports backup/rollback and file remount. | **Real on-device crypto implementation**, protected trust root/key provisioning, entire signed ELF payload stored/loaded dynamically, multi-app transactions, recovery journal and atomic bundle replacement. The host tests use an explicitly FAKE verifier. |
| App lifecycle, per-app sandbox and actual GUI App Store | Existing ring-3 process isolation and capability enforcement; new `m2_store` returns authenticated App Store card statuses from the actual persisted inventory. | Real window-rendered store, downloads, consent dialogs, GUI input, process launching of newly installed app, per-app isolation audit. |
| Third-party sample build, run, update, remove | Existing host SDK has real signed Rust ELF QEMU v1/v2/rollback/run scenarios; new host tests exercise dependency planning, persistent metadata and rollback. Dedicated M2 CI reruns R030 real QEMU package and R028 real networking demos. | Independently produced third-party packages, signed guest ingestion, app launch from post-install persistent bytes, app-by-app compatibility list. |

## New modules

- `rust/kernel_core/src/m2_sdk.rs`: bounded catalog and dependency resolution with missing-dependency/cycle detection and versioned ABI.
- `rust/kernel_core/src/m2_inventory.rs`: persistent metadata records with signed-request verification hook, exact permissions, anti-downgrade, backup/rollback and post-reboot remount tests. A trusted `RuntimeHealth` implementation is required to confirm a successfully running upgrade before releasing its rollback checkpoint. If a write fails **after** the backup record was persisted but before the active record changed, explicit owner-authorized `recover_staged_backup` removes the stale checkpoint so upgrades can resume. A failed or interrupted remove can be retried. Tests inject a failure precisely after backup metadata commit, remount the disk, refuse unsafe automatic cleanup of genuine rollback checkpoints, verify authorization and retry the upgrade. The record does **not** contain or install ELF application bytes; actual crypto and runtime-health checks must come from separate trusted guest implementations.
- **M2 package dependency acceptance:** `install_from_catalog` resolves the trusted catalog dependency graph and refuses cyclic/missing prerequisites, uninstalled prerequisites and installed versions older than the catalog requirement **before any disk writes**. Direct `install` refuses packages with dependencies so callers cannot bypass the check by mistake. This new host-tested guard is not a substitute for signed on-device catalogs or a general dynamic package loader.
- `rust/kernel_core/src/m2_network.rs`: authenticated local-port leases, conservative DNS-A parsing and fail-closed TLS-verifier hook. The real network packets are validated by the existing opt-in R028 path, not by these policies.
- `rust/kernel_core/src/m2_store.rs`: UI model backed by catalog and live persistent inventory, with PID-generation/expiry checks; no graphical store window is claimed.
- `test-m2-developer-alpha` Cargo feature and `.github/workflows/m2-developer-alpha.yml` run the test and no_std compile, the SDK's real signed-ELF QEMU test and networking QEMU test.

## Package recovery completion boundary

The package **metadata recovery** now supports interrupted-upgrade reconciliation,
trusted runtime confirmation and successive upgrades, plus idempotent cleanup
of an interrupted removal. This completes the bounded *metadata recovery
subtask*; it does not make on-device ELF installation, real signature crypto,
or M2 generally complete. The injected test health provider does NOT substitute
for a running IndianOS guest or its trusted supervisor.

## Acceptance

```sh
cargo test --offline --manifest-path rust/kernel_core/Cargo.toml --features test-m2-developer-alpha m2_
cargo check --offline --manifest-path rust/kernel_core/Cargo.toml --no-default-features --features test-m2-developer-alpha
python3 -m unittest discover -s tests -p 'test_r030_pkg.py' -v
make test-r030-package-qemu
make test-r028-network-qemu
```

The last two QEMU tests need the usual Linux/OVMF developer prerequisites.
All test disks/keys/ELFs are disposable, with NO physical-disk writes.
Do not call a green M2 host policy check a complete network stack,
production crypto implementation, actual app store, or complete M2.

**Full M2 exit gate remains:** networked third-party native apps are signed,
installed from disk, executed and updated entirely within an installed
IndianOS guest without developer workarounds, with proper user consent,
sandboxing, runtime lifecycle, and recovery; the real GUI store works.

## Persisted reverse-dependency safety

The new M2P2 inventory format persists each directly required dependency and
its exact minimum version. An authorized uninstall is denied while installed
dependents still need the target package; rollback is denied if it would
violate an installed dependent's persisted minimum. These checks survive a
disk remount and fail before any write. Legacy M2PK records still decode,
but unknown dependency provenance fails closed until explicitly migrated or
removed. Catalog/package IDs are bounded to 1..99.

Host regressions cover remount, dependency-safe removal and rollback, and
the conservative legacy path. **This is metadata consistency, not complete
M2:** no actual on-guest Ed25519, persistent executable payload ingestion,
graphical App Store, production TCP/TLS stack or third-party E2E acceptance.

## Signed ELF payload persistence and load-time verification (current development slice)

- `m2_elf.rs` adds strict, no-allocation static ELF64/x86-64 validation:
  bounds, program headers, executable entry, segment flags, memory limits,
  unsupported program-header types, writable+executable rejection.
- `m2_inventory.rs` adds opt-in `install_image`,
  `install_image_from_catalog`, and privileged `verified_image`.
  The **M2P3** record stores the exact signed ELF payload, signature and
  catalog dependency metadata in a **single** checksummed copy-on-write file.
  A crash cannot publish image metadata without its image bytes.
- Upgrading an image backs up the whole exact prior image, not only metadata;
  rollback restores the former signed image. Remount, tampering, stale
  PID generation, invalid ELF and metadata-only loading have tests.
- `verified_image` re-runs the trusted signature+digest verifier on the
  stored bytes before copying to a privileged native loader. The test
  verifier is **NOT** production cryptography. No image executes here.
- The current filesystem bank supports just **1,870 payload bytes per app
  image**; larger binaries and dynamic libraries need a bigger transaction-safe
  executable storage subsystem before complete M2.

**Still not M2 100%:** verified guest Ed25519 implementation and protected
trust keys, user-approved installation, executable mapping and scheduler
launch from those persisted bytes, arbitrary-sized third-party ELF, real GUI
Store, full TCP/DHCP/TLS, physical-hardware/security validation. No direct
physical-disk writes are introduced.


## Signed on-disk image → detached ring-3 memory staging

- `m2_launch.rs` uses `Inventory::verified_image` to read saved ELF bytes
  through the persistent CRC-checked filesystem and re-check their signed
  digest using the configured **trusted** `Verify` implementation.
- Validates the exact ELF with the already-existing `program::plan_elf64`
  and uses the existing `indianos_user_image_load` PMM/VMM API to map
  file-backed code/data and the non-executable ring-3 stack in detached
  user-owned pages. Mapping follows W^X protection. No scheduler/IRET,
  PID grant or running process is claimed.
- Fixes the earlier M2 static sample ELF to satisfy actual kernel-loader
  offset-to-vaddr alignment and strict file-backed executable-entry checks.
- Adds explicit CI host tests with a memory-backed mock PMM and no_std
  cross-check for the combined `test-m2-developer-alpha,user-image` feature
  set. Tests ensure unsigned or stale-owner input allocates **zero frames**,
  successful images contain exact persisted code and teardown releases pages.
- The unit-test signature verifier is fake; genuine on-guest Ed25519 with
  protected trust roots is still mandatory before production or arbitrary
  third-party execution. A supervisor invoking this API owns its backend
  and must not reuse the image until detached kernel CR3 is restored.

**Still pending:** scheduler integration to actually run a stored native app
in ring3, production on-guest crypto/trust keys, signed user consent, broad
real ELF application sizes (current image 1870 bytes), complete app store GUI,
and general sockets/TCP/TLS. Do not label M2 complete.

## Trusted process-table/scheduler registration integration

The M2 `prepare_registered` entry point now builds on `stage_verified`
with the existing native Rust `Process` table and actual process scheduling
APIs. It accepts a **kernel-created NEW PCB** and trusted generation,
revalidates the installed ELF via the configured signature provider, maps
it through the real PMM/VMM image loader, and transitions NEW → READY,
binding the page-table root to the exact PCB. An initialized scheduler can
then select that PCB. Host acceptance verifies the scheduler READY-selection
and native bookkeeping START/EXIT/REAP transitions with mapped pages, and
checks invalid generations/expired INSTALL rights/signature failures allocate
**no pages** or runnable process state.

This remains a **pre-execution** integration. A native scheduler selecting
a PCB and calling bookkeeping START does not execute ring-3 instructions.
A trusted runtime must still actually switch CR3, initialize user registers,
perform the IRETQ transition, handle syscalls and teardown after restoring
kernel CR3. The loader's test signature verifier remains a FAKE,
and no end-to-end guest execution of newly installed disk bytes is claimed.

CI repair: QEMU dependency installation is given retries and a bounded
10-minute download/install window with --no-install-recommends. This keeps
the mandatory M1 graphical QEMU test and M2 native guest checks enabled,
and addresses the earlier apt exit code 124 timeout.

## Installed signed executable → existing CPU ring-3 runtime (next slice)

- `m2_launch::verified_for_execution` revalidates the on-disk installed
  image signature, ELF format and *NEW* PCB before any guest CPU handoff.
  It never treats a caller-provided "verified" flag as sufficient.
- `m2_launch::execute_installed` (compiled **only** for bare-metal x86-64
  with `test-m2-developer-alpha,user-runtime`) invokes the already-used
  `x86_64_user_process_run` on exact verified on-disk bytes, not a test
  allowlist. The existing runtime performs PMM/VMM ELF staging, protected
  supervisor CR3 cloning, IRETQ ring-3 transition, int80/exit/fault handling,
  teardown, and PCB reap. This avoids duplicating privileged transitions.
- Host tests assert the disk bytes handed to the CPU path match the signed
  bytes and reject missing trust, stale or expired rights, and non-NEW PCBs.
  Guest `no_std` feature-combination compilation is a required CI step.

**Not yet an end-to-end QEMU execution proof:** no boot-time supervisor
currently invokes this entry point with a hardware-backed image and trusted
on-guest Ed25519 verifier. The existing R030 QEMU tests exercise separately
signed host SDK samples, not execution from M2P3. Real guest cryptography and
trust keys, opt-in on-disk M2P3 seed/boot harness, full QEMU execution and
rollback acceptance, larger ELF payloads, graphical App Store and general
DHCP/TCP/TLS remain pending. This implementation is a bounded **runtime
entry point**, not a completed M2 milestone.
