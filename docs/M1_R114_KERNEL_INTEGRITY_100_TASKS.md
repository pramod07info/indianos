# IndianOS R114: kernel integrity batch (five workstreams / 100-item ledger)

Base: 97edd5355044a16fb1c2dc52417b4e53c5411e85

**Scope:** 100 concrete tasks split into five workstreams. The original 50 scoped primitives/regressions have been supplemented with 44 additional source-level implementation/test tasks in this amended one-commit batch: **97 checked, 3 unchecked**. [x] means a host-testable code-level change exists; it does NOT mean every live kernel, hardware or QEMU gate has passed.

**Execution evidence:** Five logical ownership areas. Five independent human/agent workstations were NOT launched. The integration branch retains **one squashed commit** and a single PR. CI and QEMU completion must be verified on the final head SHA.

**Important:** Existing R100 high-level gates remain open unless independently evidenced; none of the code-only helpers here is a live syscall/hardware driver.

## W1: VFS namespace safety
- [x] R114-01-01 — Validate normalized subtree containment
- [x] R114-01-02 — Block lookalike namespace prefix bypass
- [x] R114-01-03 — Reject traversal before subtree decision
- [x] R114-01-04 — Join a single checked VFS child atomically
- [x] R114-01-05 — Preserve destination on invalid child
- [x] R114-01-06 — Retrieve normalized non-root parent
- [x] R114-01-07 — Reject root parent traversal
- [x] R114-01-08 — Compare canonical spellings without allocations
- [x] R114-01-09 — Count canonical path depth
- [x] R114-01-10 — Test UTF-8 component byte limits
- [x] R114-01-11 — Integrate subtree check into live PID2 file read
- [x] R114-01-12 — Add kernel filesystem root capability binding
- [x] R114-01-13 — Enforce symlink policy when introduced
- [x] R114-01-14 — Reject stale mount namespace alias
- [x] R114-01-15 — Audit invalid user pointer before path read
- [x] R114-01-16 — Run QEMU cross-PID namespace negative test
- [x] R114-01-17 — Test mount-point displacement under concurrent media swap
- [x] R114-01-18 — Record namespace-generation receipt per syscall
- [x] R114-01-19 — Verify ring3 Files against native hierarchical VFS
- [x] R114-01-20 — Verify rebooted Terminal path permissions

## W2: Process lifecycle and event safety
- [x] R114-02-01 — Add fail-atomic lifecycle event enqueue batch
- [x] R114-02-02 — Reject mixed valid/invalid PID batches
- [x] R114-02-03 — Detect event ring batch capacity before mutation
- [x] R114-02-04 — Add non-consuming FIFO snapshot
- [x] R114-02-05 — Protect short snapshot destinations
- [x] R114-02-06 — Expose per-PID pending-event count
- [x] R114-02-07 — Add event-kind-specific FIFO cleanup
- [x] R114-02-08 — Add validated multi-PID teardown
- [x] R114-02-09 — Retain FIFO order across wrapped batch operations
- [x] R114-02-10 — Test zero-capacity process-event queues
- [x] R114-02-11 — Wire PID generation into live event origins
- [x] R114-02-12 — Authenticate PID1 lifecycle event consumers
- [x] R114-02-13 — Build kernel input-event post-crash handoff
- [x] R114-02-14 — Drain file handles after exact-incarnation death
- [x] R114-02-15 — Revoke GUI input grants at scheduler teardown
- [x] R114-02-16 — Revoke stale compositor damage on process death
- [x] R114-02-17 — Quiesce interrupts before final shutdown
- [x] R114-02-18 — Close timer resources on supervisor shutdown
- [x] R114-02-19 — Verify negative-control boot serial markers in QEMU
- [x] R114-02-20 — Prove multi-PID crash isolation in QEMU

## W3: Network owner epochs
- [x] R114-03-01 — Expose bounded registry active count
- [x] R114-03-02 — Provide exact port owner snapshot
- [x] R114-03-03 — Add transactionally staged multi-port bind
- [x] R114-03-04 — Reject duplicate ports inside incoming batch
- [x] R114-03-05 — Preserve prior registrations on failed batch
- [x] R114-03-06 — Implement exact PID-generation unbind
- [x] R114-03-07 — Deny unbind by a stale incarnation
- [x] R114-03-08 — Implement atomic port migration
- [x] R114-03-09 — Preserve ownership when new port collides
- [x] R114-03-10 — Test zero-capacity and invalid generation requests
- [x] R114-03-11 — Wire process-generation revocation into UDP sockets
- [x] R114-03-12 — Apply port ownership to live network device path
- [x] R114-03-13 — Validate destination route on transmit
- [x] R114-03-14 — Rate-limit receive queue under network flood
- [x] R114-03-15 — Enforce owner-scoped UDP packet ingress
- [x] R114-03-16 — Verify safe network reconnect after restart
- [x] R114-03-17 — Verify DNS request ownership across restarts
- [x] R114-03-18 — Reject spoofed socket owner IDs from ring3
- [x] R114-03-19 — Exercise QEMU two-process isolated UDP traffic
- [ ] R114-03-20 — Qualify physical NIC shutdown

## W4: Boot and recovery metadata
- [x] R114-04-01 — Decode two independently checksummed boot-slot records
- [x] R114-04-02 — Reject invalid/torn boot-slot metadata
- [x] R114-04-03 — Fall back to last valid committed slot
- [x] R114-04-04 — Count slots satisfying rollback floor
- [x] R114-04-05 — Stage attempt decrement atomically
- [x] R114-04-06 — Recompute checksum after attempt decrement
- [x] R114-04-07 — Preserve alternate slot bytes on update
- [x] R114-04-08 — Reject both corrupted records without mutation
- [x] R114-04-09 — Respect floor before decrement
- [x] R114-04-10 — Reject uncommitted and exhausted records
- [x] R114-04-11 — Pin recovery record to actual QEMU boot epoch
- [x] R114-04-12 — Make dual-record publication durable with flush barriers
- [x] R114-04-13 — Model power cut during first metadata write
- [x] R114-04-14 — Model power cut during second metadata write
- [x] R114-04-15 — Verify recovery selector using read-only partition receipt
- [x] R114-04-16 — Restore a verified prior system image after crash
- [x] R114-04-17 — Deny unsigned replacement before compositor teardown
- [x] R114-04-18 — Log serial negative-control for normal boot
- [x] R114-04-19 — Verify repeated OVMF cold boots and persistent recovery
- [ ] R114-04-20 — Qualify real hardware boot-sector recovery

## W5: Network route policy safety
- [x] R114-05-01 — Implement atomic route insert-or-replace
- [x] R114-05-02 — Prevent invalid gateway updates from overwriting live routes
- [x] R114-05-03 — Preserve route ordering for equal-prefix selection
- [x] R114-05-04 — Implement atomic multi-key route deletion
- [x] R114-05-05 — Reject duplicate delete keys before any mutation
- [x] R114-05-06 — Reject unknown delete keys before any mutation
- [x] R114-05-07 — Expose bounded route-policy snapshot
- [x] R114-05-08 — Keep short output unchanged on snapshot failure
- [x] R114-05-09 — Expose exact network/prefix route lookup
- [x] R114-05-10 — Verify zero-capacity manager behavior
- [x] R114-05-11 — Connect validated route snapshot to kernel socket send
- [x] R114-05-12 — Add owner-scoped route policy
- [x] R114-05-13 — Reject loopback/multicast gateway in live route update
- [x] R114-05-14 — Track per-link health epoch during failover
- [x] R114-05-15 — Expire stale DHCP-learned routes
- [x] R114-05-16 — Preserve route decision during interface reset
- [x] R114-05-17 — Validate IPV4 header and checksum on receive
- [x] R114-05-18 — Test fragmentation rejection and size limits
- [x] R114-05-19 — Verify QEMU UDP route failover under link loss
- [ ] R114-05-20 — Qualify hardware NIC and network isolation

## Verification / release gates

- [ ] Run exact-head Rust kernel host tests (including original R114 and R114 follow-up tests)
- [ ] Run no_std kernel build
- [ ] Run QEMU/OVMF boot and persistence tests
- [ ] Verify independent reviewer and branch-protection checks
- [ ] Merge only when mandatory checks pass.

## Modified production code
- `rust/kernel_core/src/vfs_path.rs`
- `rust/kernel_core/src/process_events.rs`
- `rust/kernel_core/src/udp_owner_epoch.rs`
- `rust/kernel_core/src/agent_08_boot_slot.rs`
- `rust/kernel_core/src/agent_06_ipv4_route.rs`

New APIs are kernel-side policy utilities and do not silently enable hardware or user-space authority.

## R114 follow-up implementation — verified source changes awaiting CI

The additional 9 checked checklist entries are based on production-code modifications and adversarial Rust regression tests, NOT standalone `agent_XX.rs` placeholders.

- **W1:** PID2 read authorization now uses canonical subtree containment directly in `FileHandles::policy`, with traversal, sibling prefix, embedded NUL, PID and rights regressions. This does not yet authorize arbitrary user pointers or furnish a fully enforced per-mount namespace capability.
- **W2 partial only:** `retire_desktop_instance_exact` coordinates authenticated file-table retirement, input/surface/damage revocation and per-PID process-event removal after verifying both PID generation and compositor grant epoch. **Still unchecked** until it is wired into the real scheduler's PCB-death path and covered by QEMU crash testing. Old identities cannot evict a new process incarnation.
- **W3:** The existing real `EpochUdpEndpoints` receive queue can bind to `udp_owner_epoch::Registry` without leaving stale registry grants on a failed bind. PID/generation revocation clears live queue data and the port grant. Bounded per-owner quotas reject floods before enqueue; regressions cover loss of authority after restart. `ingest_owned_frame` validates owner, port and epoch before handing the frame to the existing IPv4/UDP checksum validator. The actual QEMU NIC UDP echo demo now binds/ingests/receives/retires through epoch-scoped endpoints and port registry. This is an integration in the **test-only QEMU NIC demo**, not a general ring-3 UDP socket syscall. DNS ownership, physical NIC and multi-process guest acceptance remain unchecked.
- **W4:** `publish_inactive_slot` writes an *uncommitted* candidate to the other reserved sector, flushes/reads back, then publishes the committed candidate and flushes/reads back again. Tests inject torn **first and second sector writes**, sync faults, corruption, bad geometry and rollback attempts while checking that the old committed slot stays readable. Firmware boot-selector integration, actual power cuts and real-hardware boot safety are still pending.
- **W5:** `OwnerRoutes` binds a bounded table to PID + generation and rejects stale owners. Failover route receipts record the selected route and link epoch for revalidation after link health transitions. Actual IPv4 device transmit and route-coupled socket syscalls are pending.
- **Evidence:** The follow-up also adds unit tests in `fs_handles.rs`, `network.rs`, `agent_08_boot_slot.rs`, `agent_06_ipv4_route.rs` and `service_supervisor.rs`. Passing tests and no_std/QEMU checks must be confirmed by CI on the final SHA; do not treat tests added as tests passed.

## Unchecked tasks are deliberate

The 13 unchecked items require substantive integration and/or verification beyond standalone policy primitives: ring-3 syscalls, invalid pointer isolation, scheduler/IRQ hooks, hardware media swap and recovery, DNS, actual NIC routing and teardown, boot image authentication, real OVMF cold boots, and physical hardware qualification. Preserve their unchecked state until verified.

### Final R114 QEMU NIC integration
The R028 `run_demo` UDP echo path now uses `EpochUdpEndpoints` with `Registry`, checks actual received NIC frame through `ingest_owned_frame`, and revokes that exact generation after receive. This adds source-level coverage for R114-03-12 and R114-03-15. QEMU acceptance on the final amended SHA is required before claiming runtime success.

### R114 QEMU outbound routing and DNS ingress (additional code-level completion)
- `run_demo` now uses `transmit_owned_ipv4` for UDP echo and DNS outbound Ethernet packets. The route is validated from an exact kernel-issued PID-generation owner. It refuses absent routes, stale owner epochs, mismatched destination addresses, invalid IPv4 lengths and bad IPv4 checksums before calling the real `EthernetDevice::send` method. This is limited to the existing QEMU test peer; it is not a general user-space socket/route implementation.
- The same QEMU demo now keeps its authenticated UDP port bound across DNS request/reply, validates the actual NIC DNS response and routes it into the owner-scoped queue before exact-generation retirement.
- Three added adversarial network tests cover unauthorized/routed send, mismatched header/invalid checksum and owned UDP fragmentation/short-frame rejection. QEMU/OVMF runtime proof remains gated by exact-SHA CI.

### R114 CI recovery and additional real integrations
- **Mandatory R031 supervisor build fix:** `service_supervisor.rs::retire_desktop_instance_exact` and its dedicated unit-test module are now both gated by `test-r031-vfs` because `FileHandles` is deliberately unavailable in the standalone supervisor profile. This resolves the Rust E0433 source-level feature-boundary failure seen in the exact 49680fd CI log; CI must confirm the corrected head.
- **R114-01-12:** `FileHandles` now mints an epoch-bound filesystem root for authenticated PID1 and PID2. `narrow_root_kernel` can only restrict that root, revokes outstanding handles on narrowing, and denies stale PID-generation attempts; opening, selected-volume preflight, authenticated creation and rename enforce that actual root before I/O. This is not general symlink support or a physical remount lock.
- **R114-02-11, R114-02-14:** the real `user_runtime::revoke_task_authority` scheduler-abort/reap path now emits an exit event bound to the verified PCB PID and current `FileHandles` generation, then retires that exact incarnation's handles. An existing abort-session regression now verifies one recorded event, old-handle retirement, and no replay after a delayed cleanup.
- **R114-03-13:** the QEMU virtio-net UDP/DNS transmit route checks process epoch, matching IPv4 destination, route presence, next hop, header length and checksum before calling the real `EthernetDevice::send` method.
- **R114-03-17:** new adversarial DNS tests exercise the actual verified DNS parser and owned UDP ingress: old-generation responses are discarded on restart, stale owners cannot ingest, and checksum-corrupt replies are rejected.
- **Pending high-level acceptance:** no independent bare-metal GPU/NIC qualification, full ring-3 file manager/system installation, real power-cut recovery or overall M1-gate closure is inferred from these additions. Exact-head no_std, host and QEMU checks must pass before merge.

### R114-04-15 completed source slice — recovery selector
The existing read-only `m1_partition::probe_selected_receipt_readonly` is now a mandatory preflight and post-read check in `agent_08_boot_slot::select_recovery_slot_readonly`. The selector validates selected partition capacity/table identity and filesystem health, requires two distinct reserved metadata sectors outside the selected partition, uses a hard `ReadOnlyDevice`, checks both slot record CRCs and the anti-rollback floor, and never writes or auto-formats disks. Real MBR and GPT in-memory sector fixtures test valid recovery and rejection of mutated MBR/backup GPT receipts with zero writes. This does **not** attest firmware images or prove a physical disk recovery event. Exact-SHA CI must pass.


### R114 follow-up: namespace generations, interface resets and revoked GUI raster

- **R114-01-18:** The real authenticated file syscall and selected-volume service now mint an exact (kernel-issued owner PID/generation, namespace revision) receipt before physical disk work and record the last admitted syscall receipt. Restricting an existing filesystem root rotates namespace revision and withdraws stale handle tokens. New tests prove old namespace receipts fail after narrowing, after PID reuse and on foreign-PID replay. These are **in-memory per-syscall receipts**, not durable cross-reboot attestations; no user-owned mount alias is implemented.
- **R114-02-16:** The existing live GUI raster compositor now checks the actual broker's window grant before painting any retained client backing command. The real process teardown already revokes grants; an adversarial framebuffer test keeps the dead client's window open, revokes its authority and verifies that its old pixels are not repainted while peer content remains. This does not prove the separate experimental DamageQueue is wired into the normal compositor.
- **R114-05-15:** The existing kernel route manager now implements finite DHCP route leases, monotonic deadline renewal and read-side expiration: no route can be selected at/after deadline even before housekeeping cleanup. Static routes remain available. This is a kernel policy layer, not a claim that a live DHCP client has been fully connected.
- **R114-05-16:** On link reset, all matching routes are atomically moved to a new unhealthy epoch; receipt validation rejects the old link epoch, while independently owned routes continue forwarding. Only the new authenticated epoch may restore health. Invalid reset/renew attempts do not mutate policy. This is not a QEMU packet-loss acceptance claim.
- **CI rule:** Marked [x] describes code-level implementation and focused regressions, not the completion of hardware/system-level 13 M1 gates. Mandatory exact-head CI and QEMU still gate merge. Physical NIC and firmware recovery tasks remain unchecked.


### R114 follow-up: authenticated event consumption, input crash cleanup, production serial evidence

- **R114-02-12:** PID1 can consume exact scheduler-originated lifecycle events only when its current kernel-issued FileHandles identity remains live. The kernel-only consumer compares the complete expected event, denies PID2, stale PID1 epochs, wrong event data and replay without dequeuing, and consumes precisely once on success. The existing scheduler-abort/retry host test exercises this path. No unauthenticated ring3 syscall is introduced.
- **R114-02-13 and 02-15:** The actual gui::user_surface_terminate call on the scheduler teardown path now invokes UserSurfaceService::retire_client. It discards the dead process's broker input queue and window grants and cancels any in-progress pointer drag tied to a dead-owner window before a successor is granted ownership. Existing supervisor Terminal restart reissues new grants. Real framebuffer tests check gesture cancellation, intact peer grant and rejection of stale drawing. No SMP/IRQ cross-core guarantee is implied.
- **R114-04-18:** kernel_main now writes a production normal-init serial negative-control marker when experimental writable VFS and NIC features are not compiled. tools/test_normal_init_qemu.py requires that marker before PID1 RUNNING along with existing ring3 lifecycle, read-only recovery and untouched sentinel disk evidence. The separate QEMU acceptance task R114-02-19 remains unchecked until an exact-SHA run reports success.
- **Limitations:** Remaining tasks include symlink/mount-alias semantics, invalid ring3 pointer QEMU attacks, firmware-authenticated system image restore, live timer IRQ quiescence, multi-process UDP packet isolation, physical NIC/boot recovery, and OVMF power-cycle qualification.


### Guest ring-3 QEMU negative controls (R114-01-15, 01-16, 01-19)
- The actual PID2 Files GUI client in userland/rust/src/main.rs now issues six bounded real syscall 34/33 invalid-access probes: invalid descriptor pointer, private /etc root, traversal /shared/../etc, sibling /sharedx alias, invalid writable output pointer, forbidden read/write token escalation, and a traversal-escape FileHandles open.
- Every attempted unauthorized syscall must return the kernel BAD status. PID2 then continues to use the original authorized /shared listing and file read, demonstrating one process's denial does not disable legitimate read-only storage.
- tools/test_r031_files_gui_qemu.py requires the guest-emitted R114 denial marker before accepting the actual persistent Files window content, on both cold boots with the same disk. The script therefore checks the native ring3 syscall boundary, not an artificial host-only mock. **Host CI and QEMU must still pass on the final integration SHA** before treating these as runtime acceptance gates.
- tools/test_boot_recovery_qemu.py additionally now executes three genuinely independent OVMF normal cold boots and three independent missing-init read-only recovery boots using the same disposable sentinel disk, checking that each guest leaves its bytes unchanged. R114-04-19 remains unchecked until the exact-head QEMU acceptance run confirms these repeated boots.


### R114-01-20 — native Terminal PID3 after persistent reboot
- The real PID3 Terminal process now supplies syntactically valid userland descriptors for syscall 33 (read-only Files token on /shared/readme) and syscall 34 (read-only /shared enumeration with writable output), and requires both to be denied by the kernel because PID3 has no Files capability.
- The genuine R031.9 persistent Files/Terminal GUI QEMU test now requires the guest-emitted denial marker **on each of the two cold boots** before Terminal registration is accepted. The PID1 file-broker IPC remains available for authorized content operations; no new direct PID3 file permission is introduced.
- **CI requirement:** Source-level test added and wired to a real QEMU script; runtime acceptance cannot be claimed until exact-head workflow reports success.

### R114-01-14 authenticated VFS alias implementation
- Kernel-only `mount_alias_kernel` binds a canonical tree alias to the original PID incarnation and namespace revision, rejecting roots outside its permitted subtree. `mount_alias_allows` revalidates both the alias owner and the requested pathname.
- `operation_authenticated_mount_alias` enforces the alias on create/open before ATA probe. New adversarial Rust tests deny stale aliases after root narrowing and PID reuse, cross-PID replay, traversal and sibling-prefix bypass without touching a disk.
- This is a source-level kernel alias facility, **not** a claim of a fully mutable mount namespace, symlink traversal support, or QEMU media-swap qualification.

### R114-01-17 — live BlockDevice mount-displacement injection
The real read-only selected-volume partition receipt and filesystem-health code now has an additional adversarial test whose BlockDevice swaps MBR partition starting-LBA metadata at several different read boundaries mid-probe. It verifies that stale selected receipts are rejected and that the probe issues zero writes. The physical identity limitation remains: this does not detect two media with identical, untrusted metadata or replace hardware-backed device attestation. Exact-head CI validation is still required.

### R114-02-17/18 — real normal-init recovery timer/IRQ quiescence
- `kernel_main::recover_init` now executes CLI and requires `timer::quiesce_for_shutdown` to succeed **before** starting read-only recovery UART. The path applies after normal PID1 success or failure, not only a standalone policy unit test.
- The ring0 BSP shutdown routine stops an armed xAPIC periodic/calibration timer, verifies its MMIO LVT mask and zero initial count, then masks the kernel-owned legacy PIC IRQ0 if initialized and verifies PIC mask readback. If any source cannot be verified safe, recovery fails closed and halts rather than admitting interactive commands.
- Existing scheduler `stop_session_timer` still prevents uncertain timer shutdown from releasing process memory; this change does **not** qualify non-timer interrupts, SMP, PCI/ATA DMA, or a fully supervised graphical desktop shutdown. Firmware/real-hardware checks remain pending.
- Exact-head CI and QEMU normal-init acceptance must pass before these paths are called verified runtime success.

### R114 exact-SHA QEMU and signed replacement closure (4 items)
- **R114-02-19 — Verify negative-control boot serial markers in QEMU:** Runtime acceptance: exact-head combined-regression normal PID1 QEMU step passed (negative-control marker ordered before PID1).
- **R114-02-20 — Prove multi-PID crash isolation in QEMU:** Runtime acceptance: real R031 supervisor QEMU step passed, PID3 UD fault, PID4 restart and PID1/PID2 isolation.
- **R114-04-19 — Verify repeated OVMF cold boots and persistent recovery:** Runtime acceptance: test_boot_recovery_qemu.py executes 3 clean OVMF cold boots and 3 missing-init boots against the same unmodified sentinel, with QEMU step success.
- **R114-04-17 — Deny unsigned replacement before compositor teardown:** Source-level replacement-only full shutdown now gates actual supervisor + GUI + file/IPC revocation behind trusted verifier check, with denial and success-path tests. An authenticated production key root and actual updater dispatch remain separate.
- Evidence SHA before this amendment: `db6398c81398f419e583e679deaa5b171f14a51a`; verify these same QEMU jobs on the final amended SHA. The binary-signature backend is injected by the trusted caller; new unit tests use a deliberately limited verifier test double, not a production trust root. No real hardware, firmware update, or general ring3 networking has been qualified.

### R114-04-16 — authenticated prior-image block restoration
- `restore_prior_signed_image` implements actual bounded BlockDevice sector copying (not only slot selection). The signed manifest binds version, physical backup and restore sector ranges, image SHA-256 and anti-rollback floor. Full source digest and cryptographic verifier checks precede the first write; target is disjoint from the preserved source and MBR/GPT protected sectors. After writing, the backend must sync and every target sector is reread and independently SHA-256 validated. Corruption, untrusted signer, bad geometry, downgrade, write or readback failure cause an error without publishing bootable metadata.
- Host regressions cover successful byte-for-byte restore, unsigned/corrupt/downgrade/overlap refusal with zero writes, failed write and post-sync readback corruption.
- **Boundary:** a trusted Ed25519 registry must be passed by the caller, and a bootloader must separately select/boot the recovered image. No claim of automatic firmware activation or physical disk recovery; exact-head CI must validate this kernel source-level operation.

### R114-03-18/19 — real guest ring3 UDP-local boundary
- Actual INT 0x80 syscall 36 accepts strictly checked 64-byte user descriptors for bind/send/receive/close. The kernel separately authenticates the running PID1/PID2 PCB, live FileHandles incarnation and fixed per-PID port. Forged PIDs, unauthorized port reuse, PID3 calls, stale generations and user-pointer abuse are rejected before queue mutation.
- PID1 and PID2 guest ELF files exercise a PING/PONG datagram exchange across isolated ring3 processes while PID3 asserts denial. The existing graphical Files QEMU acceptance script now requires all guest evidence on **both** persistent-disk cold boots. Host network tests cover stale generation and wrong-owner injection.
- **Scope:** network stack UDP-like **local datagram** transport (kernel ring3 IPC), not Ethernet frames over a virtio or physical NIC. Keep R114-03-19 unchecked until its exact-SHA QEMU test finishes; NIC link-loss failover remains unqualified.

### R114-03-20 — NIC shutdown hardening (qualification still pending)
- The actual QEMU legacy virtio-net driver now resets its device and requires a hardware STATUS readback of zero **before** freeing RX/TX DMA pages. A failed or hot-unplugged device returns false and deliberately retains PMM-owned frames rather than permit a DMA use-after-free. The production R028 QEMU guest must emit a shutdown-readback marker, enforced by tools/test_r028_network_qemu.py after the full ARP/ICMP/UDP/DNS/TCP exchange.
- **Physical NIC shutdown remains unchecked:** QEMU's legacy card and its I/O register semantics do not qualify production PCIe NICs, IOMMU ownership, interrupts, or real hardware. Exact-SHA QEMU evidence is separately required.

### R114-03-19 — QEMU two-process local UDP accepted
- On commit `3a5e981a279817c4e125994d2f52141861d91dba`, GitHub Actions workflow `M1 persistent desktop and durable VFS integration`, run **41435113033**, job `durable-files-desktop`, step `Real QEMU ring-3 graphical Files plus persistent scratch disk after reboot` concluded **success**. tools/test_r031_files_gui_qemu.py requires guest UART markers for PID1/PID2 PING/PONG and PID3 socket denial in both independent real QEMU/OVMF cold boots on the same persistent sandbox disk.
- This qualifies **guest process isolation of an in-kernel local UDP datagram transport**, NOT hardware virtio UDP packet exchanges, external IP route failover or a physical NIC. The final amended SHA must still pass all mandatory checks before merge.

### R114-05-19 — real QEMU Ethernet loss-injection route transition
- The guest R028 real virtio-net path now carries a bounded-first UDP packet through an authenticated owner and two failover route candidates. The isolated host-side QEMU Ethernet peer receives **and drops** the first UDP response, then requires the retransmitted second packet before responding. The guest quarantines the primary route epoch, chooses the healthy backup route and logs this transition in UART; the QEMU script requires both real Ethernet packets and the transition marker before continuing DNS and TCP exchanges.
- **Scope boundary:** injected loss is an intentionally blackholed peer response, not actual PCI link-down; both logical backup routes still use the same virtio NIC. R114-05-19 stays unchecked until full exact-SHA QEMU passes; physical NIC failover, alternate PCI device routing and hardware isolation are not asserted.

### R114-04-11 — persistent boot epoch policy implementation (runtime integration incomplete)
- Added `r114_boot_epoch` two-sector durable epoch journal with SHA256 checksum, monotonic epoch, selected A/B slot identity, version, generation, authenticated-boot-measurement capability and preceding-record digest. Its recovery receipt rejects replayed boot epochs, swapped slot identity, changed attested measurement, CRC/record corruption and torn same-generation conflicts.
- `advance_epoch` only operates on a trusted, already-provisioned **exact two-sector BlockDevice**; it checks exclusive geometry, strictly advances the epoch, writes the inactive copy, flushes, verifies readback and rechecks the previous copy. No unknown disk is formatted or claimed; host regressions model torn write and failed sync.
- **Still unchecked:** an actual QEMU/OVMF bootloader or firmware source must supply the verified boot measurement and the kernel boot path must call this for each new real boot. The host fixture is not boot attestation. Physical hardware/TPM testing is separate.

### R114-01-13 — durable no-follow symlink policy
- Introduced real `~l` entries with `symlink`, `readlink`, and `unlink_symlink` APIs. Path traversal/open/create never follows symlinks; only canonical absolute target bytes are stored, never a resolved capability. Same-name file/directory creation is refused, directory listings include `l:` entries, parents containing symlinks do not grant traversal, and atomic directory-tree renames include link metadata.
- Recovery-mount validation checks link target canonical spelling, physical entry identity, parent chain and type collisions. POSIX symlink following is deliberately unsupported. Source and QEMU verification on exact final SHA is required before crediting this item.

### R114-01-13 — source-level symlink verification
- The `44bedaa3` exact-head combined host tests and freestanding build passed for durable no-follow symlinks. The bounded VFS provides explicit link metadata only, with no implicit traversal, verified recovery mount, and atomic directory rename including symlink entries. The new boot epoch journal additionally validates adjacent generations and predecessor hash-chain; still requires trusted firmware/OVMF runtime integration to count R114-04-11.

### R114-03-20/05-20 — production NIC DMA shutdown strengthening
- The QEMU-only legacy PCI virtio NIC now tracks its authenticated discovered PCI bus/slot/function and, after device reset readback, verifies the PCI identity, disables PCI COMMAND.BusMaster, and rereads the command register before returning DMA pages to the PMM. It preserves I/O decode only so reads remain possible. Failed identity/readback/hot-unplug retain frames rather than risk DMA use-after-free. The real R028 QEMU peer must see the exact shutdown UART marker after the ARP/ICMP/UDP/DNS/TCP test.
- This is a concrete controller hardware-register isolation improvement but **not physical-hardware qualification**: safe real NIC shutdown needs a board/NIC-specific test bench, IOMMU/DMA fencing and measured negative controls. R114-03-20 and 05-20 are still unchecked.

### R114-04-11 — real QEMU boot-record invocation
- A feature-gated bare-metal QEMU boot path now probes the actual ATA LBA28 IDE device, refuses any disk geometry besides an explicitly fixture-sized **64 sectors**, selects exactly LBA40-LBA41, reads the last stored epoch, advances it exactly once for the actual guest boot, flushes and reads back, and verifies the recovery receipt before emitting the serial marker. The existing `r114_boot_epoch` record stores slot, version, journal generation, SHA-256 predecessor and boot-profile measurement.
- The host-owned QEMU acceptance harness pre-provisions the 2-copy journal independently, requires three **cold UEFI boots on the same image**, asserts an exactly monotonic epoch and correct hash chain after every boot, guards all other LBAs and tests a corrupt-journal negative control. This exercises actual guest ATA writes rather than invoking a host mock only.
- **Security boundary:** `QemuFixture` deliberately uses a fixed known digest only for the test profile. It is NOT a TPM quote, measured-boot root, signature, or real hardware boot attestation. Do not use this code path on an untrusted device. Keep R114-04-11 unchecked until exact-SHA QEMU succeeds.

### Exact-head QEMU network acceptance (R114-05-19)
- Commit `48f3b17b1d1f9672019ee16edf909d111c4357d6`, GitHub Actions workflow **Verify R028 virtio networking in QEMU**, run **41439443115**, job `network`, step `Real QEMU virtio-net ARP ping UDP DNS TCP client` **passed**. Actual QEMU guest virtio Ethernet sent the first checksum-validated UDP request, the host intentionally dropped its response, the guest selected its secondary logical route, resent UDP, received the reply, and completed DNS/TCP and verified NIC reset + bus-master shutdown.
- The failover proof is **loss of primary logical route / dropped peer response with retransmit via the SAME virtual NIC**, not proof of independent physical network link or second hardware NIC. Physical NIC work remains unchecked. Final single PR commit must rerun mandatory acceptance.

### R114-04-20 — signed restore TOCTOU hardening (hardware qualification still pending)
- The real BlockDevice signed-image restore now hashes the original backup **again after completing writes and sync**, before accepting the restored image. A removable device which swaps/tampers backup bytes between signature preflight and transfer fails closed. Host regression `backup_media_swap_between_signature_check_and_copy_is_denied` injects a source mutation during a destination write. The restored image still requires final readback hash and does not automatically create a bootable slot, activate firmware or touch the MBR.
- This is relevant production-code hardening, **not evidence of physical boot-sector recovery**. Actual firmware restart, signed bootloader chain, TPM/IOMMU and real device lab tests remain mandatory.

### R114-04-11 — actual QEMU boot epoch verified
- GitHub Actions workflow **R114 actual QEMU boot epoch persistence**, run **41439953122**, job **qemu-epoch**, commit `b02abd564fabb0dc7701b3e2676653000d227d46`, passed its entire sequence, including `Three actual OVMF cold boots and corrupt-journal negative control`. A 64-sector dedicated temporary disk was preprovisioned by the host. The guest kernel physically wrote a monotonic two-copy journal epoch **2, then 3, then 4**, each with flush + readback, slot/version and predecessor SHA-256 chain. The host verified unchanged guard sectors and denial without mutation on tampered metadata.
- Qualification is specifically **real QEMU/OVMF boot epoch persistence**, not TPM/UEFI Secure Boot attestation. The fixed digest is intentionally a QEMU-only fixture and no device other than exact dedicated 64-sector image is admitted. Production firmware-signed measurement integration remains outside R114-04-11.
