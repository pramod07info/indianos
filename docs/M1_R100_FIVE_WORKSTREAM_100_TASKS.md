# IndianOS R100 — Kernel-first five-workstream implementation ledger

## Status and accounting
- **Target:** 100 real engineering tasks, five logically separate workstreams × 20 each.
- **Actually implemented or newly evidenced in this PR:** 82 of the 100 source-level deliverables; **18 pending**. R113 adds three new code-level deliverables plus an evidence-backed audit that closes eight existing real QEMU/OVMF/PS2/boot implementation items. This does NOT assert physical hardware qualification or all 13 M1 high-level gates. Live ring-3/hardware acceptance remains open. These source-level checks do not substitute for real QEMU/hardware power-loss acceptance.
- **New Rust tests:** 117 focused cases across durablefs, fs_handles, gui_broker, service_supervisor, m1_partition, hier_vfs and batch6_boot (18 R100 + 11 R101 + 15 R102 + 13 R103 + 3 R104 + 8 R105 + 4 R106 + 6 R107 + 4 R108 + 10 R109 + 10 R110 + 5 R111 + 4 R112 + 6 R113). The actual M1, no_std, QEMU and ELF/ISO suites must pass on the exact amended SHA.
- **Five actual independent coding agents:** not launched. Work is divided into five code-ownership areas and consolidated by one coordinator workflow; no fictitious agent execution.
- **Merge policy:** one integration commit, one PR; do not merge before all mandatory CI checks pass and branch protections are satisfied.
- The remaining high-level M1 gates in [#616](https://github.com/pramod07info/indianos/issues/616) must pass their **own live QEMU acceptance**. This ledger does not mark any of those gates complete.
- No physical disks are formatted; tests use in-memory/disposable QEMU media.

## Full 100-task list, exact implementation state

### Workstream 1: G2 Partition discovery and mount safety
**Owned source:** m1_partition.rs
- [x] K01-01 — Post-mount revalidation of checked partition
- [x] K01-02 — Post-mount revalidation of strict healthy mount
- [x] K01-03 — Route default partition mount through checked path
- [x] K01-04 — Pin raw MBR identity across mount
- [x] K01-05 — Capture partition table digest before mount
- [x] K01-06 — Reject changed MBR signature on remount
- [x] K01-07 — Reverify GPT disk GUID after COW scan (full table receipt pinned before and after mount)
- [x] K01-08 — Reverify GPT partition GUID after COW scan (full table receipt pinned before and after mount)
- [x] K01-09 — Detect duplicate live GPT partition GUIDs across both valid mirrored arrays (recomputed CRCs, read-only regression)
- [x] K01-10 — Reject inconsistent primary/backup GPT entry arrays before selected-volume probe/mount
- [x] K01-11 — Reject same-GUID partition geometry relocation even when both GPT arrays/headers have valid updated CRCs
- [x] K01-12 — Reverify full mirrored GPT entry/header receipt after COW health reads; reject same-GUID/same-LBA partition attribute changes
- [x] K01-13 — Reject hybrid protective GPT plus live legacy MBR volume without fallback or writes
- [x] K01-14 — Reject selected physical device if its original exact capacity changes (receipt pinned across later mount)
- [x] K01-15 — Reject unexpected GPT primary/backup header/table read failures before mount without writes
- [x] K01-16 — Protect selected MBR diagnostics with a whole-device read-only capability and pre/post-table receipt checking
- [x] K01-17 — Record selected partition provenance for recovery
- [ ] K01-18 — Verify reboot GPT GUID binding in QEMU
- [x] K01-19 — Verify post-mount driver media swaps via fault injection
- [ ] K01-20 — Qualify physical disk read-only discovery policy


### Workstream 2: G6 Durable filesystem and power-cut recovery
**Owned source:** durablefs.rs
- [x] K02-01 — Retire COW mount after uncertain data-bank write
- [x] K02-02 — Retire COW mount after uncertain metadata publication
- [x] K02-03 — Retire COW mount after failed flush
- [x] K02-04 — Retire COW mount after geometry drift
- [x] K02-05 — Quarantine bank on checksum failure (retire entire suspect mounted session)
- [x] K02-06 — Reject cross-mount stale metadata generation at every mutation (and now every verified data read)
- [x] K02-07 — Persist explicit fsync cache barrier (strict opt-in durable file writes; legacy writes retain existing contract)
- [x] K02-08 — Verify two-head COW durability after simulated power cut (host fault injection; actual QEMU cut pending high-level gate)
- [x] K02-09 — Survive torn first data sector (host block-failure injection + remount)
- [x] K02-10 — Survive torn last data sector (host block-failure injection + remount)
- [x] K02-11 — Survive torn primary metadata head (host block-failure injection + remount)
- [x] K02-12 — Survive torn backup metadata head (host block-failure injection + remount)
- [x] K02-13 — Recover valid old generation without partial paths (strict namespace-integrity recovery mount after COW rollback)
- [x] K02-14 — Reject a lying device which acknowledges dropped writes
- [ ] K02-15 — Cross-process filesystem ownership lock
- [x] K02-16 — Recovery-mode read-only mount by explicit user choice (explicit read-only mounting API, user-facing recovery selector still pending)
- [x] K02-17 — Authenticated directory rename after reboot (PID1 epoch-bound tree rename and clean remount verification)
- [x] K02-18 — Authenticated selected-volume file unlink after reboot with token retirement and fresh-mount namespace check
- [x] K02-19 — Two genuine OVMF/QEMU boots use same disposable IDE disk; updated /shared/readme survives remount and is rendered by Files
- [x] K02-20 — Document and test safe recovery from dead disk (transient read fails without exposing data; every retry must revalidate heads and payload, persistent corruption retires mount)


### Workstream 3: G7 Authenticated ring-3 file syscall operations
**Owned source:** fs_handles.rs
- [x] K03-01 — Authenticated positioned read preserving file cursor
- [x] K03-02 — Authenticated positioned write preserving file cursor
- [ ] K03-03 — Kernel-only handle token issuance
- [x] K03-04 — Reject stale PID generation before selected physical volume I/O, including post-rebind token reuse
- [x] K03-05 — Revoke all file descriptors on process exit (exact-incarnation restart coordinator)
- [ ] K03-06 — Reject invalid user pointer before disk I/O
- [x] K03-07 — Prevent output-buffer leaks after failed selected-device reads with negative block-fault injection and unchanged caller bytes
- [x] K03-08 — Keep PID2 read-only /shared scope enforced before any selected physical volume I/O
- [x] K03-09 — Enforce protected OS path write denial before selected physical volume probing/mount
- [x] K03-10 — Kernel-authorized narrowing of one service-wide write budget; consumed credits persist across selected-volume handle reopen and PID1 generation rebind
- [x] K03-11 — Rebind checked volume after reboot (kernel-authenticated receipt-pinned file operations)
- [x] K03-12 — Atomic handle-based rename with authorization
- [x] K03-13 — Atomic checked file unlink
- [x] K03-14 — Optimistic generation-guarded write
- [x] K03-15 — Flush through authenticated fsync ABI
- [x] K03-16 — Quarantine selected file-service session after uncertain COW write, revoke all tokens and require explicit kernel-authorized strict remount of same physical receipt
- [ ] K03-17 — Independent ring-3 Files create/read/write
- [ ] K03-18 — Independent ring-3 Terminal modify/read
- [ ] K03-19 — Cross-process negative isolation in actual QEMU
- [ ] K03-20 — Read-after-power-cycle via authenticated syscall


### Workstream 4: G4/G5 Compositor and physical input routing
**Owned source:** gui_broker.rs
- [x] K04-01 — Reject overflowed broker event sequence batches
- [x] K04-02 — Atomically validate owner-bound physical input batch
- [ ] K04-03 — Route PS2 keyboard IRQ to authenticated input broker
- [ ] K04-04 — Route PS2 mouse packet to owner-bound pointer service
- [ ] K04-05 — Verify hardware keyboard parity/timeouts
- [x] K04-06 — Prevent foreign focus takeover
- [x] K04-07 — Guard pointer capture by owner and grant epoch
- [x] K04-08 — Release focus and pointer capture on verified process exit
- [x] K04-09 — Revoke old GUI capability generations
- [x] K04-10 — Reject input to dead process
- [x] K04-11 — Drain per-owner input without cross-PID leakage (verified grant epoch and exact owner/window)
- [x] K04-12 — Deliver balanced key down/up in order
- [x] K04-13 — Preserve peer FIFO and monotonic input sequence across exact-epoch window recovery and regrant
- [x] K04-14 — Kernel-side compositor broker/surface admission is one staged atomic lifecycle transaction
- [ ] K04-15 — Reject foreign framebuffer mapping
- [x] K04-16 — Only publish compositor damage after exact broker-grant and surface-epoch admission with full rectangle clipping and fail-atomic queued submission
- [ ] K04-17 — Enable independent ring3 compositor
- [ ] K04-18 — Support Files focus with keyboard navigation
- [x] K04-19 — R047 QEMU monitor sendkey via emulated PS/2 (not UART) causes actual screenshot/pixel-delta changes across two boots
- [x] K04-20 — Verify mouse capture loss after epoch-verified app crash, with input, damage and surface cleanup (kernel-side integration; physical IRQ QEMU acceptance pending)


### Workstream 5: G3/G8 PID1 service supervision and boot evidence
**Owned source:** service_supervisor.rs
- [x] K05-01 — Authorize service crash using exact PID and generation
- [x] K05-02 — Stage group restarts atomically without partial quota consumption
- [x] K05-03 — Exact-head runtime QEMU normal-init build (without test-only runtime feature flags) enters real PID1 and returns safely to recovery
- [x] K05-04 — Real graphical QEMU startup reports independent PID3 Terminal, verifies active service and reaps PID3 at controlled shutdown
- [x] K05-05 — Real graphical QEMU startup displays PID2 Files with live persistent filesystem content and reaps PID2 at shutdown
- [ ] K05-06 — Launch independent compositor ring3 process
- [x] K05-07 — Prepare IPC grants before service activation (supervisor verification policy; live PID1 path still pending)
- [x] K05-08 — Deny crash replay of retired PID without quota/capability mutation
- [x] K05-09 — Reject invalid/unsigned replacement image *before* desktop teardown; after real crash stay Degraded until independently verified image is staged
- [x] K05-10 — Reject PID generation wraparound in restart admission/retry capacity and never reuse wrapped epochs
- [x] K05-11 — Bound restart admission and expose only actually usable service-specific budget credits
- [x] K05-12 — Release dead service file handles (trusted restart coordination hook; live scheduler wiring still required)
- [x] K05-13 — Release dead service GUI grants (trusted restart coordination hook; live scheduler wiring still required)
- [x] K05-14 — Stop all services with exact-list shutdown and kernel-side file, IPC, input, surface and frame-damage teardown (hardware quiesce still pending)
- [x] K05-15 — Kernel-only versioned/redundantly stored recovery record with checksum, monotonic boot epoch and remount/corruption tests
- [ ] K05-16 — Graceful timer and interrupt shutdown
- [ ] K05-17 — Serial negative control for normal boot
- [x] K05-18 — OVMF pflash + IndianOS ISO boots two distinct QEMU processes against the same disposable IDE disk and completes service checks
- [x] K05-19 — QEMU pc/OVMF IDE ATA disk boundary guards, PS2 keyboard via sendkey and mouse packet desktop routing pass; emulated hardware only
- [x] K05-20 — Two distinct QEMU cold starts with serial PID1/PID2/PID3 lifecycle checks and second-boot persisted file content verification


## Evidence requirements
Every checked item above represents modified **existing production Rust source**, not a checklist-only claim. For accepted task completion, compile the code on this PR's exact SHA and verify the affected host/no_std, adversarial FS, authenticated native FS, persistent desktop, real QEMU, and kernel ELF/ISO serial boot workflows. Correct failures in production code; never disable tests to make CI green.

**Delivery constraint:** The 100-item milestone is not done until all unchecked tasks have substantive code changes plus acceptance evidence. Do not convert 82 source-level implementations/evidence into “100 tasks completed” via test counts or documentation.

## R101 implementation additions (after the original R100 baseline)
- **K01-04, K01-06**: `MbrSelection` pins the exact 512-byte MBR header and the selected partition; `inspect_mbr_selection` checks identity across discovery; `mount_mbr_selected_healthy` compares it both before and after mounting. This is a *fingerprint*, not a hardware-unique cryptographic device ID.
- **K02-06**: data reads now check both recorded COW metadata heads before copying verified bytes. A changed head is latched as a permanently retired mount; explicit new mount is required. This extends stale-write protection to stale-read protection.
- **K04-10, K04-12**: `DesktopInput::keyboard_batch` and `click_batch` use validated current compositor focus/capture bindings and staged input queues to avoid partial key pairs or pointer notifications. Existing physical IRQ driver wiring and independent compositor PID are **still pending**.
- **Additional partial groundwork only (not marked done)**: `ServiceTable::all_active_exact` checks distinct authenticated service identities for trusted boot coordination; `shutdown_all_exact` atomically stops a complete set of authenticated service slots. Actual system shutdown and PID1 normal boot are not implemented here and K05-03/K05-14 remain unchecked.
- **11 additional focused unit tests** cover swapped MBR headers, remount, stale data readers, input owner isolation, queue capacity, live service readiness and epoch-controlled shutdown. No tests disabled.
- **Outstanding:** 82 numbered production tasks remain unchecked; the eight high-level M1 acceptance gates must still be demonstrated by real QEMU/hardware evidence.

## R102 — Implementation additions and CI acceptance limitations
- **K01-05, K01-17 (G2):** `PartitionReceipt` stores the selected partition, exact MBR sector zero, mirrored GPT unique GUIDs and CRC32 over complete validated primary/secondary GPT table/header sectors. `inspect_partition_receipt` verifies a stable snapshot; `mount_partition_receipt_healthy` checks it before and after the mounted filesystem metadata reads. CRC32 is *not* a cryptographic device identifier. Read-only discovery does not write physical media.
- **K02-16 (G6):** `mount_recovery_readonly` wraps the physical block device with a hard `ReadOnlyDevice` capability; even an accidental privileged mutation through the returned filesystem cannot reach a write sector. It is a deliberate API, not an auto-recovery boot selector.
- **K02-07 (G6):** `create_durable_path`/`overwrite_durable_path` opt into explicit `BlockDevice::sync()` barriers: new COW data sectors → flush → metadata head publication → flush → recheck heads. A missing/failed flush retires the mount. Existing legacy write paths remain independently qualified; this does not prove all OS writes are power-loss durable.
- **K03-15 (G7):** authenticated `FileHandles` action 27 performs checked `HierVfs::sync()` only on a current WRITE handle, returning verified filesystem generation without mutation budget charge. A failed flush retires the token.
- **K04-06, K04-09 (G4/G5):** `DesktopInput::focus_authenticated` rejects foreign focus requests; `window_closed` revokes precisely the requested grant, purges pending events, clears focus/capture and prevents stale grant-epoch replay.
- **K05-12, K05-13 (G3/G8):** `restart_and_revoke_exact` validates the supervisor and file-handle PID/generation before scheduling a replacement, then revokes old process handles and compositor grants before that replacement can be prepared. This is **kernel-side coordination API**; full normal PID1 scheduler integration is still pending, and no M1 acceptance gate is marked complete.
- R102 tests: 3 partition receipts + 2 strict COW flush + 3 authenticated fsync + 3 GUI focus/window lifecycle + 3 cross-service process revocation + 1 hard read-only recovery test = **15 new Rust tests**. Total 44 new Rust tests across R100–R102.
- **Remaining 67 numbered source tasks** are deliberately left unchecked. Live ring-3 processes, physical input, real QEMU reboot acceptance and physical driver support require end-to-end code and CI artifacts.

## R103 — Exact-head CI recovery and five additional production areas

**CI recovery:** The earlier R102 runtime-QEMU failure was a genuine freestanding feature-configuration error: the supervisor-only kernel compiled `service_supervisor.rs` without `fs_handles` enabled. The cross-module restart/FS cleanup now compiles only when `test-r031-vfs` is active, and its related tests follow the same feature contract. The supervisor-only profile remains independent and must pass CI on the exact integration SHA.

### Six newly checked R103 numbered tasks
- **K01-19 (G2):** an injected MBR media swap *during* mounted metadata reads changes boot code while preserving partition geometry. The checked partition receipt must detect it, refuse mount, and perform zero writes.
- **K02-14 (G6):** strengthen strict COW writes by verifying every target payload sector **after** the data-cache flush and **before** publishing the new metadata head. Reject and retire the session when a controller acknowledges writes but discards them at its own purported sync; old committed bytes remain recoverable.
- **K03-13, K03-14 (G7):** authenticated file-handle syscall action **30** unlinks exactly the selected current-generation file, retires its handle and charges S0 quota on success; action **28** permits generation-checked writes without stale-cursor or quota side effects. Additional action **29** creates a new file through kernel-authorized PID1 ownership, normalized path and S0 write budget. Native full ring-3 Files executable acceptance is still pending.
- **K04-11 (G4/G5):** compositor event receipt is scoped to the precise kernel-issued **grant epoch**, window and PID. A short buffer, foreign owner or recycled grant cannot consume any messages. A peer's events are never drained.
- **K05-07 (G3/G8):** `ServiceTable::prepared_with_ipc_and_window` gates replacement-service activation on a registered **exact PID/generation** `ScopedRouter` IPC receiver **and** current broker window grant epoch. An unprepared or recycled process cannot activate. This is an explicit kernel-side integration policy; actual normal boot PID1 startup wiring remains unverified.

**New R103 Rust tests (13):** MBR media swap 1, lying storage flush 1, authenticated create/delete/generation 5, owner/epoch-scoped input delivery 3, IPC and window-prepared service 3. No tests disabled; all remain subject to exact-head CI compilation, QEMU boot and negative regression evidence.

**Unfinished 61 items:** Includes full normal-boot ring-3 Terminal, Files and user compositor, physical IRQ/device integration, driver qualification, filesystem lock and real power-loss/reboot matrix. Do not declare those complete based on unit tests or the number of Rust functions.

## R104 — Authenticated metadata-atomic file rename (K03-12)
- New authenticated file-handle **action 31** renames only the current WRITE handle's file to a separately validated destination. The destination must normalize, must not cross a protected S0 boot/recovery namespace, must not already exist, and the handle's pinned filesystem generation must be current.
- The on-disk rename is one metadata-atomic `HierVfs::rename_file` COW transaction. Successful rename charges the existing shared write budget once and revokes the old source handle. I/O or uncertain metadata failure retires the token instead of allowing stale pathname replay.
- Added three Rust regression tests for successful remount-persistent rename, foreign PID/protected-path/traversal denial, and existing-target collision without quota or namespace mutation.
- Total: **34/100 source-level deliverables implemented; 66 pending; 60 new Rust tests**. This does **not** close normal-boot ring-3 Files or physical QEMU acceptance gates.

## R105 — Storage fail-closed recovery and complete service-capability retirement
- **K02-05:** a CRC mismatch in a file's mounted data-bank payload now quarantines the *entire* mounted COW session (`retired` latched). Even if a corrupted sector looks valid again later, neither subsequent reads nor writes can reuse it without explicit remount/revalidation. Output buffers remain unchanged.
- **Additional failure fix (not a second checked item):** any uncertain write, readback or post-repair health verification in `repair_redundancy` irreversibly retires that mounted session. Retry is permitted only with a newly created mount and fresh checksum/identity checks; no filesystem tests disabled.
- **K03-05, K05-08:** `restart_and_revoke_full_exact` verifies the exiting PID/generation has an exact IPC mailbox *before* attempting the supervisor restart. On the one authorized transition, it clears the old process's file handles, window grants, focus, pointer capture, queued inbound IPC and outbound IPC messages held by peers; a replay of the already retired PID is denied without consuming further restart budget. This is an integration API for the trusted kernel scheduler; normal PID1 boot wiring is still pending.
- **K04-07, K04-08:** `DesktopInput::capture_pointer_at_epoch` and `release_pointer_at_epoch` require exact PID/window/grant-epoch authorization, so delayed release from an old client cannot clear the next client's capture. Existing process-exit cleanup is exercised by the crash integration and new tests, preserving peer grants and preventing post-crash pointer delivery.
- **8 new R105 unit regressions:** 2 COW repair/corruption, 3 integrated IPC/GUI/file-handle lifecycle, 3 exact-epoch pointer ownership/capture. Overall **68 added tests**, **39 implemented ledger tasks**, **60 still pending**. QEMU, live ring-3 and hardware acceptance remain independently required.

## R106 — Selected-volume authenticated file API and cross-media handle isolation
- **K03-11:** `operation_on_selected_volume` authenticates the process incarnation **before accessing the device**, verifies the complete selected `PartitionReceipt` and strict healthy COW volume on each file operation, and dispatches through the kernel handle table. It does not autoformat or pick the first physical disk.
- A newly introduced `selected_volume: Option<PartitionReceipt>` in `FileHandles` pins the authenticated service's open handles to one exact selected on-disk MBR/GPT identity. On a different volume, any live old descriptor rejects access **before touching the target device**, even when the other volume has the same filesystem generation and pathname. A successful new open/create may bind a different checked volume only when all old handles are retired.
- Four R106 regression tests: selected remount file read, stale process generation denial before disk I/O, stale changed-MBR refusal before write, and wrong-volume handle replay rejection with explicit close/volume rotation. This establishes source-level reboot rebind semantics but **does not** imply real QEMU power-cycle acceptance or independent ring-3 Files startup.
- **Ledger at R106:** 40 source deliverables implemented, 60 still unchecked; 72 new Rust tests across R100–R106. See R107 for current totals. No physical disks formatted and no failing tests suppressed.

## R107 — GPT table-session pinning and cross-checkpoint power-loss recovery
- **K01-07, K01-08:** the public `mount_gpt_selected_healthy` now captures a `PartitionReceipt` for the **entire validated primary and backup GPT structures** and protective MBR, then rechecks it after COW volume-health and mounted metadata reads. This is stronger than comparing disk/partition GUID + LBA alone: a same-GUID, same-geometry table/boot metadata swap cannot bypass identity checks. GPT receipts are CRC32 integrity fingerprints, not hardware-unique authentication.
- **K02-08 to K02-12:** host block-device fault injection tears the **first and last file-bank payload sectors** and independently the **primary or backup metadata head** at the exact write boundary. On simulated power loss, the uncertain writer retires, a fresh mount chooses the previously verified checkpoint, and the prior filename, bytes and generation remain unchanged. This is deterministic **host-side simulation**, not proof of unmodified QEMU hardware power cuts.
- **K02-20:** transient failed reads of COW metadata or live file payload fail without exposing bytes or publishing writes. A subsequent operation must independently reverify both recorded metadata heads and the complete payload; an actual checksum mismatch, geometry drift, uncertain write/flush, or changed head permanently retires that mount. This honors existing authenticated file-handle retirement behavior and explicit recovery policy.
- **6 new R107 Rust regressions:** verified healthy selected GPT mount, GPT MBR-media-swap during COW health, first/last payload sector tears, both metadata-head tears, transient failed header read revalidation, and transient failed data read revalidation. The two grouped tear tests exercise **four distinct crash boundaries**, not just one case.
- **R107 count: 48/100 source-level deliverables implemented; 52 pending; 78 new Rust regression tests** across R100–R107. See R108 for latest totals. CI must pass on the *exact new integration SHA*. Existing high-level M1 acceptance gate #616 and real serial QEMU evidence remain separate. No code disables tests, formats physical disks or declares real ring-3 boot complete.

### R107 exact-head CI regression correction
- Original R107 SHA `2aad4c9395628cb57dda60db9860140419a8f8ba` failed four pre-existing host tests. Root cause: the first R107 variant permanently retired a mounted filesystem for **all transient sector-read errors** (even when both metadata heads remained unchanged). This broke retry after removable-media I/O recovers, a previously supported filesystem behavior.
- Corrected behavior: a failed read returns `FsError::Io` and preserves output, filesystem metadata, sector contents, and quota. The following operation revalidates both on-disk metadata heads before touching payload or making a write; a changed head/checksum, geometry drift, or uncertain write still retires the session. Existing failed-I/O FS-handle tokens independently retire and must be reopened. Both R107 new read-error tests now verify clean retry only after revalidation.
- CI must rerun the same unforked existing test suite, including `m1_repair_and_health_propagate_payload_io_without_writing`, `m1_r053_failed_block_read_retires_handle_before_retry`, `m1_r056_faulted_read_does_not_publish_device_dirtied_bytes`, and `directory_payload_read_fault_keeps_listing_and_media_unchanged`.

## R108 — Namespace-integrity recovery and authenticated directory-tree rename
- **K02-13:** `HierVfs::mount_consistent_recovery` mounts the latest verified COW checkpoint but does **not** expose it until `listing_integrity()` has verified every reserved directory/file key, canonical parent chain, directory marker, and file/directory alias conflict. It never formats or repairs a physical disk. A torn newest metadata head must expose the previous **complete directory tree**, not a partial renamed tree. A checksum-valid but orphaned flat COW object must be rejected before the hierarchical filesystem can be used.
- **K02-17:** kernel-only authenticated Files **action 32** performs an atomic rename of a directory and all descendants using `HierVfs::rename_dir`'s COW metadata publication. It requires an exact live PID1 process generation, kernel-normalized source and target under the unprotected `/shared/` subtree, S0 authorization, and write budget. On success, every file handle scoped to the moved subtree is retired immediately; unchanged unrelated tokens continue to face the regular filesystem-generation authorization check. Reboot/remount must read the complete tree at the new path and reject the old path.
- Four additional Rust tests cover latest checkpoint torn-head fallback; rejection of COW-valid orphaned hierarchical filenames; full tree rename after fresh remount with old-token revocation; foreign/forged PID, protected-target and traversal denial without writes or quota.
- The prior R107 read-error correction preserves existing transient-I/O recovery tests. Error returns do not leak output and any retry fully rechecks metadata/payload; permanent retirement remains mandatory for uncertain writes, diverging checkpoint bytes, geometry drift and checksum corruption.
- **R108 count: 50/100 implemented source-level deliverables, 50 pending; 82 new focused Rust regressions.** See R109 for current totals. No independent QEMU/hardware or unimplemented ring-3 service gate is marked complete.

## R109 — Pre-disk security authorization, physical capacity pinning, restart-credit safety and compositor crash cleanup
- **K03-04, K03-08, K03-09:** `FileHandles::preflight_selected` is a kernel-owned, allocation-free authentication boundary on the existing *authenticated selected-volume* entrypoints. It verifies the exact live process PID/generation and opaque handle token before any ATA probe/MBR/GPT read, rejects PID2 WRITE and access outside `/shared/`, and denies protected OS/boot/recovery writes, traversal aliases and forged directory-rename inputs before device I/O. Every operation repeats its existing authorization once the checked volume is mounted. The legacy generation-zero kernel-owned compatibility API is not claimed to be ring-3-safe; real user pointers still need syscall copy-in validation.
- **K01-14, K01-15:** the full `PartitionReceipt` now includes exact `device_sectors` and checks it across the full discovery/digest sequence and each later selected remount. Replacing a disk by one with the identical MBR partition bounds but a different physical capacity fails before mounting. GPT primary/backup header and table read faults fail closed, leaving disk write counters unchanged. This is a capacity-integrity measure, not cryptographic device authentication.
- **K05-10, K05-11:** process-supervisor admission `can_restart_service`, pending retry census, per-service remaining credits, and total advertised credits now exclude services at `u32::MAX` process generation. `crash_exact` and pending retries still use checked arithmetic, consume bounded credits and refuse PID collisions without allowing reactivation of a wrapped incarnation. An independently healthy peer remains active if a process runs out of attempts.
- **10 new Rust regression tests:** 3 selected-volume pre-I/O auth / token / epoch tests; 2 physical capacity & primary/backup GPT error tests; 3 restart epoch and bounded-credit tests; 2 atomic GUI input/capture/surface/damage cleanup tests. No tests disabled or bypassed.
- **K04-20:** trusted `retire_crashed_compositor_owner_exact` validates the precise broker owner/window/grant epoch and live surface **before any mutation**. It then revokes the crashed process's input grants and pending notifications, focus, pointer capture, framebuffer surface authority, and dirty-frame queue in one kernel-serialized transition. A delayed crash from an old epoch cannot evict a replacement or destroy another user's surface. This is a kernel integration API; live physical mouse IRQ/QEMU replay still requires end-to-end validation.
- **R109 count: 58/100 source-level items implemented, 42 pending, 92 added Rust regression tests.** See R110 for latest totals. This is not 57 completed end-to-end M1 gates, and it does not claim physical storage qualification, live ring-3 isolation or QEMU boot success without exact-SHA CI evidence. Five *logical* workstreams, not independently launched developer agents.

## R110 — Read-only device diagnostics, authenticated reboot unlink and compositor lifecycle

**Six source-level deliverables:**
- **K01-16:** `probe_selected_receipt_readonly` wraps the **entire** parent BlockDevice in `ReadOnlyDevice`, verifies the complete original MBR/GPT `PartitionReceipt` before and after COW health discovery, and returns health without any formatting, repair, or media writes. A changed MBR identity fails before volume access. This tests logical read-only enforcement, not physical write-protect switches.
- **K02-18:** authenticated PID1 file-handle action **30** invoked through `operation_on_selected_volume` persists unlink on the selected partition; duplicate stale-token reuse is denied before I/O. A fresh process/file-service table after simulated reboot cannot reopen the deleted file.
- **K03-07:** a disposable selected block device that **dirties its sector output and then returns I/O error** is wrapped in exact receipt verification. Authenticated file-read operation aborts before exposing *any* changed caller bytes, performs no disk writes, and can succeed after the fault clears and a fresh volume revalidation passes. Actual unsafe user-pointer validation is still K03-06 (pending).
- **K04-13:** `Broker::recover_window_exact` verifies owner/window/old grant epoch, stages revocation and new grant, and commits only when both operations succeed. Surviving peers' queued events retain their exact FIFO and sequence IDs, while newly routed events use strictly higher IDs. Replayed stale epochs cannot clear the new owner grant.
- **K04-14:** `grant_compositor_surface_exact` stages both broker input ownership and compositor framebuffer surface authority on independent copies before committing together. Invalid geometry, duplicate window IDs, and exhausted capacity cannot leave a half-issued capability or advance grant epochs. Full independent ring-3 compositor launch remains K04-17/K05-06.

- **K05-14:** `shutdown_and_revoke_capabilities_exact` requires an exact, complete active supervisor/PID/epoch list plus registered IPC receivers and any required exact-generation file service owners **before** changing state. A successful kernel-serialized shutdown stops all active services and revokes each process's file-token table, IPC messages, GUI focus/capture/input grants, framebuffer surfaces and pending frame damage. Missing IPC ownership or stale generations refuse shutdown without partial cleanup. Physical timer/interrupt/disk quiescence is *not* included and remains K05-16.

**Ten new Rust regression tests:** two read-only MBR diagnostics, two selected-volume unlink/faulted read tests, four GUI lifecycle/input recovery tests, and two all-service shutdown/capability teardown tests. No tests disabled and no physical disk formatted.

**R110 source-level ledger: 64/100 implemented, 36 pending; 102 focused additional Rust tests across R100–R110.** See R111 for latest totals. Gate #616, full QEMU/UEFI boot, actual IRQ paths, independent Terminal/Files/compositor ring-3 processes, and external-disk acceptance are not inferred from host tests; wait for exact-head CI before making any pass claims.

## R111 — Verified GPT mirrored-table identity, safe selection and adversarial regression

**Five numbered source-level GPT gates closed:**
- **K01-09:** even a validly checksummed GPT with **two live partitions using the same unique partition GUID** is rejected. Both GPT arrays and both header CRCs are recomputed in the test; only an explicit duplicate-ID policy catches it.
- **K01-10:** if a secondary GPT array differs by one entry byte, neither discovery nor selected-volume probe can mount it or write anything; primary and backup consistency is required.
- **K01-11:** GPT partition geometry can be moved while retaining the exact disk/partition GUIDs and recomputing **both** valid header and entry-array CRCs. The prior receipt rejects that new layout before mounting the old partition or writing.
- **K01-12:** **production code changed**: `probe_gpt_identity_health` now uses a read-only parent-device capability, pins a complete `PartitionReceipt` of the protective MBR, BOTH GPT headers and full entry arrays, disk capacity, and selected GUIDs **before and after** filesystem health reads. An injected device-side swap updates partition attributes *during* the health scan with matching GUIDs/LBAs and valid CRCs and must fail closed.
- **K01-13:** protective GPT MBR with an additional live legacy MBR entry is rejected as ambiguous/unsupported; the mount never falls back to blindly mounting the legacy partition.

**Five new focused Rust regression tests**, including valid-CRC GPT duplicate-ID, inconsistent mirror, hybrid layout, table relocation, and fully valid attribute-swap-while-health-reading. Existing GPT and MBR tests remain enabled. These are read-only software and disposable block-device tests, **not** proofs of physical disk qualification or true power-off behavior.

**R111 source-level count: 69/100 implemented, 31 pending; 107 new focused Rust regressions across R100–R111.** See R112 below. Exact-head host/no_std/runtime QEMU/ELF ISO validation must still succeed, and the eight M1 acceptance gates are separate.

## R112 — Torn selected-write quarantine and exact-epoch framebuffer damage

- **K03-16:** the selected-volume file-service now persists `selected_quarantined` after a failed mutating operation (`Io` or `InvalidVolume`) on a strictly verified mounted partition. Because disk controllers can partially write a data bank before reporting failure, the service revokes **all** file tokens (including surviving read-only peer tokens) and rejects subsequent opens, reads and writes *before touching media*. Closing/rebinding process handles does not clear the flag. Privileged `revalidate_quarantined_selected_volume` requires a current kernel-issued PID1 epoch, the exact prior `PartitionReceipt`, zero surviving handles, and a full healthy MBR/GPT+COW remount before the service can issue fresh tokens. Service-wide write budget and monotonically issued token IDs survive; a failed first create binds its verified receipt for subsequent recovery. A stale identity, changed MBR or a different-capacity disk cannot unlock the quarantine.
- **K04-16:** `DamageQueue::submit_verified_at_grants` authenticates exact owner/window with **both** the kernel-issued broker grant epoch and compositor surface epoch, before admitting a bounded list of rectangles. Each rectangle is checked against the owned framebuffer surface geometry with checked coordinates; invalid, out-of-range, unowned, recycled or full-queue requests publish **no** partial frame, epoch or authority. Submitted frames carry their verified grant/surface generations; `present_fair_verified` drops old work after surface/window recycling. This is a kernel-side damage authority boundary, not proof of user-mode framebuffer mapping restrictions (K04-15 remains pending).
- **Four new Rust regression tests:** torn selected-disk mutation that returns error after actually modifying a physical sector, requiring explicit healthy recovery; denial for replayed PID1/changed receipt; grant-recycle damage replay denial and verified current presentation; foreign/oob/full-queue damage batch rejection with no partial work. Hardware/IRQ/ring-3 acceptance remains independent.

**R112 total: 71 of 100 source-level tasks implemented, 29 unchecked; 111 additional focused Rust tests across R100–R112.** R113 current totals follow. No tests bypassed, no physical disks formatted, and no actual power-loss or independent ring-3 completeness is claimed until relevant QEMU/hardware gates demonstrate it. Exact-head CI must be green before merge.

## R113 — Real boot image preflight, persistent recovery diagnostics, shared write budget

- **K05-09:** `Bootstrap::restart_failed_desktop` now verifies the proposed desktop image is signed/verified, has a nonzero image ID and an in-range nonzero entry point **before** tearing down a live desktop or revoking any of its process/window grants. Previously a malformed replacement could be rejected only after the old desktop was retired. A genuinely crashed desktop remains in `Degraded` recovery until a verified replacement is staged, without consuming retry credits on rejected image attempts. Two tests exercise unsigned, invalid-entry, nil-image and genuine-crash recovery. This is kernel boot policy; normal-boot ELF image validation and actual ring-3 launch remain separate acceptance gates.
- **K05-15:** kernel-only `persist_boot_recovery_record` writes a fixed-size, versioned, checksum-protected status record at `/recovery/boot-state` on an **already explicitly mounted** redundant COW volume, recording monotonically increasing boot epoch, actual degraded/stopped state, failure code, PID1/desktop identity and restart headroom. `read_boot_recovery_record` checks exact record bytes, magic, version, phase, checksum and nonzero epoch/failure. Repeated/stale epochs and recovered-but-corrupt data are rejected; explicit COW sync is required before success. Three tests prove fresh-remount readback without writes, epoch anti-replay and rejection when a checksum-valid filesystem contains an altered diagnostics record. This is an explicit privileged boot-service hook; full automatic normal-boot wiring remains a separate high-level test.
- **K03-10:** `FileHandles::narrow_service_write_budget` accepts an exact current kernel-issued PID1 incarnation only, and delegates to the monotonic, non-widenable `WriteGuard` limits of the **same persistent file service**. Failed/foreign requests and quarantined volume cannot reset/write budgets. A selected-physical-volume integration test issues writes, closes/opens handles, rotates PID1 process generation and proves the next write is rejected **without additional disk writes** after the fixed quota is consumed.

**Six new R113 focused Rust tests**, on top of all existing host, no_std, durable Files and runtime QEMU tests. No tests disabled. No implicit formatting or physical host disk mutation.

**R113 code-only source-level tally before acceptance audit: 74/100 implemented, 26 pending; 117 added focused regression tests.** In particular, *independent* user-ring3 Terminal/Files/compositor boot, protected framebuffer page mappings, kernel user-pointer memory validation at every syscall boundary, real IRQ/mouse, strict physical ATA and UEFI reboot acceptance, and eight M1 high-level gates must still be verified independently. No claim that an entire working OS has reached 74%.

### R113 integration build failure and correction
- Exact old head `659ccb4ac45a440dbeba28b9b98795033dad6034` initially failed host combined-regression, adversarial security/no_std, runtime QEMU and ELF/ISO preflight: `batch6_boot.rs` referenced `crate::hier_vfs` while `hier_vfs` was compiled out under no-`test-r031-vfs` feature profiles. Actual Rust errors: E0432/E0433. The QEMU and ELF jobs failed at compilation or reported skipped downstream checks; this is **not** a passed QEMU boot.
- Corrected production feature gating on the two VFS-dependent diagnostic routines and their checksum/phase serialization functions, and on the three dependent Rust tests, using the exact `#[cfg(feature="test-r031-vfs")]` dependency already established by `lib.rs`. Core boot admission/image checks remain compiled and exercised in normal-init profiles. VFS diagnostics remain available in the corresponding kernel runtime profiles. No tests or CI workflow steps were disabled; incompatible source was no longer compiled in profiles where its dependency does not exist.
- Re-run required checks on corrected exact SHA; the R113 source-level tally of 74 checked / 26 pending remains provisional until CI passes.

### R113 evidence audit — existing tested implementation (8 items, no new code falsely credited)
The eight newly checked existing source-level items are **not** eight features written in R113. They have existing production code and were formerly unchecked pending acceptance evidence. Exact head `4cb24c21895b5299613c98c673e8870d6db29a0d` had `runtime-qemu` and `durable-files-desktop` **success**, including the real normal-init PID1 stage and `make test-r031-files-gui-qemu` with `tools/test_r031_files_gui_qemu.py`:
- **K02-19:** one 128-sector disposable IDE disk is shared across two separate OVMF/QEMU VM processes. The first boot creates then updates `/shared/readme`; the next reads and displays `FILE UPDATED` in the real Files window. The test checks guard sectors are unchanged and COW superblock magic remains valid.
- **K04-19:** real HMP `sendkey` (not COM1) injects PS/2 key events. Test screenshots `.ppm` after each physical action and fails on insufficient framebuffer-pixel difference, across both boots.
- **K05-03:** the separate runtime-QEMU normal-init step builds a no-test-only-profile kernel and proves PID1 execution and controlled recovery.
- **K05-04 / K05-05:** actual graphical UEFI QEMU boot verifies distinct PID2 Files and PID3 Terminal services are ready, and that both independent processes are reaped on shutdown. This is bounded QEMU profile evidence, not universal process isolation across every platform.
- **K05-18:** booted an actual IndianOS ISO under OVMF pflash firmware on QEMU twice; preserved disk and completed shutdown checks. No physical motherboard UEFI claim.
- **K05-19:** QEMU emulates ATA IDE persistence and physical PS/2 keyboard/mouse events; real virtual-8042 input path, framebuffer pixel changes and disk boundary protection were checked. Physical laptop drivers still need testing.
- **K05-20:** two independent QEMU cold starts with PID1/PID2/PID3 serial readiness/reap provenance and persisted second-boot file content.
- Evidence: [R031 real QEMU acceptance test source](https://github.com/pramod07info/indianos/blob/feature/m1-r100-gates-five-stream-implementation/tools/test_r031_files_gui_qemu.py) and exact GitHub Actions `durable-files-desktop` / `runtime-qemu` successful checks for the cited head. **This is acceptance reconciliation, not extra R113 Rust test cases.**
- **Latest tally: 82/100 source-level deliverables checked; 18 pending; 117 new focused R100–R113 Rust tests.** The remaining 18 require substantial actual implementation or stronger separate high-level gates; no auto-completion of the 13 M1 gates or physical hardware compatibility.
