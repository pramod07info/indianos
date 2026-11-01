# IndianOS R087 — Kernel-first M1 100-task integration batch

**Current state: 100 scoped, auditable work items; batch NOT complete.** No implication that 100 engineers/agents were running. Production code in this initial commit targets only the live G2 block-window safety boundary. Keep PR draft until actual integrated M1 acceptance evidence exists.

**Authority:** [M1 gate tracker #616](https://github.com/pramod07info/indianos/issues/616). M1 accepted baseline 5/13, G1–G8 pending. Prior R086's 1,200 planned items do not represent completed implementation. R087 is a focused top-100 execution subset, not 100 additional accepted capabilities.

**Gate sequencing:** G2/G6/G1/G7 storage and authenticated file roundtrip first; G3/G4/G5 normal boot with real hardware input second; G8 kernel recovery and release evidence third. Do not add unrelated AI, compatibility, UI themes, or M2 scope.

**Policy:** one consolidated integration commit and PR per batch. Real code and tests required; exact-SHA rust host, no_std, kernel/QEMU disk/reboot/keyboard evidence and reviewer sign-off before checking tasks or merging. Failed CI must be fixed, not bypassed. No physical disk writes.

## 100 prioritized tasks

### Workstream 1: G2 — Partition and block-device safety
**Source ownership:** `rust/kernel_core/src/block.rs; m1_partition.rs`

- [ ] R087-T001. Pin parent-device sector capacity at BlockWindow construction.
- [ ] R087-T002. Preflight parent capacity before every bounded read.
- [ ] R087-T003. Preflight parent capacity before every bounded write.
- [ ] R087-T004. Reject successful reads whose parent capacity changes during transfer without publishing bytes.
- [ ] R087-T005. Report an uncertain write when device capacity changes during the write.
- [ ] R087-T006. Test both device shrink and expansion with no backend I/O before transfer.
- [ ] R087-T007. Test a mid-read media-capacity change does not publish the staged sector.
- [ ] R087-T008. Test a mid-write media-capacity change returns I/O error and documents nonrollback semantics.
- [ ] R087-T009. Test an unchanged device still reads and writes inside the selected window.
- [ ] R087-T010. Validate protective MBR geometry before allowing an explicit GPT selection.
- [ ] R087-T011. Verify matching primary and secondary GPT disk identity on actual mounted media.
- [ ] R087-T012. Reject GPT entries overlapping metadata or other live partitions before selecting a mount.
- [ ] R087-T013. Revalidate the selected GPT disk and partition GUID after the final mount read.
- [ ] R087-T014. Bind selected partition to immutable expected geometry for the full mounted session.
- [ ] R087-T015. Reject a partition-selection token after device replacement or geometry rotation.
- [ ] R087-T016. Exercise malicious device read errors during MBR/GPT discovery without disk writes.
- [ ] R087-T017. Validate selected partition error handling with a disposable ATA QEMU disk.
- [ ] R087-T018. Assert partition discovery never formats or writes an unselected device.
- [ ] R087-T019. Test explicit partition selection remains stable after separate QEMU boots.
- [ ] R087-T020. Collect exact-SHA M1 G2 evidence for a real selected-partition read/write/reboot sequence.

### Workstream 2: G6 — Durable VFS and crash recovery
**Source ownership:** `rust/kernel_core/src/durablefs.rs; hier_vfs.rs`

- [ ] R087-T021. Document filesystem data and metadata write ordering for an authenticated file commit.
- [ ] R087-T022. Ensure file payload writes precede publishing new COW metadata roots.
- [ ] R087-T023. Propagate device flush or commit errors to the owning service.
- [ ] R087-T024. Keep old COW generation mountable when a new generation write fails.
- [ ] R087-T025. Detect torn superblock records before trusting metadata.
- [ ] R087-T026. Validate filename and directory entry bounds at filesystem ingress.
- [ ] R087-T027. Keep directory rename atomic across interrupted metadata commits.
- [ ] R087-T028. Reject stale directory handles after their parent generation changes.
- [ ] R087-T029. Validate concurrent file lock ownership using PID plus generation.
- [ ] R087-T030. Reject lock reuse after the original process terminates.
- [ ] R087-T031. Verify an interrupted append preserves a valid previous file version.
- [ ] R087-T032. Verify a truncated file is correct after a clean remount.
- [ ] R087-T033. Exercise disk-full handling without corrupting surviving directory entries.
- [ ] R087-T034. Reject unexpected metadata CRC mismatch rather than autoformat.
- [ ] R087-T035. Keep guest-visible durable file reads stable after service restart.
- [ ] R087-T036. Verify unauthenticated callers cannot bypass per-user file ownership.
- [ ] R087-T037. Run disposable QEMU power-cut simulation across COW commit phases.
- [ ] R087-T038. Reread the same file from a newly booted guest after clean shutdown.
- [ ] R087-T039. Reread a previous valid generation after injected failed commit.
- [ ] R087-T040. Capture exact-SHA G6 evidence for durable file, rename, crash, and recovery scenarios.

### Workstream 3: G1/G7 — Authenticated ring-3 file operations
**Source ownership:** `rust/kernel_core/src/m1_persistent_terminal.rs; syscall.rs; user_runtime.rs`

- [ ] R087-T041. Connect terminal command ingress to the active ring-3 process identity.
- [ ] R087-T042. Reject a terminal request if its kernel PID generation no longer matches.
- [ ] R087-T043. Require a live mount grant before opening the selected durable file volume.
- [ ] R087-T044. Reject terminal writes outside the authorized mounted partition.
- [ ] R087-T045. Route a real guest keystroke sequence into terminal command parsing.
- [ ] R087-T046. Make guest terminal write persist through authenticated VFS calls.
- [ ] R087-T047. Enable ring-3 file creation using kernel-validated user pointers.
- [ ] R087-T048. Enable ring-3 file reads through authenticated descriptor handles.
- [ ] R087-T049. Enable ring-3 file edits without allowing forged process identifiers.
- [ ] R087-T050. Enable ring-3 file rename with checked source and destination ownership.
- [ ] R087-T051. Prevent caller output-buffer insufficiency from partially modifying disk.
- [ ] R087-T052. Verify a read-only file capability cannot invoke disk mutation.
- [ ] R087-T053. Release file descriptor and lock ownership on process exit.
- [ ] R087-T054. Reject a recycled PID that presents a stale file handle.
- [ ] R087-T055. Ensure terminal command errors are visible without exposing other users' data.
- [ ] R087-T056. Test two independent ring-3 processes accessing the same existing file safely.
- [ ] R087-T057. Test successful guest write followed by full QEMU shutdown and new boot.
- [ ] R087-T058. Test guest readback from Files and Terminal through the same selected volume.
- [ ] R087-T059. Test filesystem error propagation across syscall and Terminal boundaries.
- [ ] R087-T060. Capture exact-SHA G1/G7 guest keyboard-to-file-to-reboot evidence.

### Workstream 4: G3/G4/G5 — Normal boot, isolation and physical input
**Source ownership:** `rust/kernel_core/src/kernel_main.rs; user_runtime.rs; m1_input.rs`

- [ ] R087-T061. Start ring-3 PID1 from the default production boot route without test-only flags.
- [ ] R087-T062. Require verified PID1 image and safe entrypoint before process publication.
- [ ] R087-T063. Launch Terminal, Files, and compositor as isolated user-mode tasks.
- [ ] R087-T064. Keep app-window composition logic out of the privileged kernel path.
- [ ] R087-T065. Map per-app graphical surfaces with explicit process ownership.
- [ ] R087-T066. Reject writing to another application's mapped display buffer.
- [ ] R087-T067. Deliver physical PS/2 keyboard scancodes through a bounded input queue.
- [ ] R087-T068. Deliver PS/2 pointer packet data through a validated input broker.
- [ ] R087-T069. Reject malformed keyboard events without interrupt-handler crash.
- [ ] R087-T070. Reject impossible pointer deltas before desktop dispatch.
- [ ] R087-T071. Enforce foreground focus when routing keyboard events.
- [ ] R087-T072. Release pointer capture when the owning application exits.
- [ ] R087-T073. Prevent one blocked app from stopping compositor service progress.
- [ ] R087-T074. Restart a failed normal boot service without recycling stale handles.
- [ ] R087-T075. Fallback to explicit recovery mode after unrecoverable PID1 startup failure.
- [ ] R087-T076. Make terminal cursor and entered command visible in QEMU framebuffer.
- [ ] R087-T077. Route a real mouse click to Files selection in the guest.
- [ ] R087-T078. Route file-edit action through the compositor into a ring-3 process.
- [ ] R087-T079. Test graphical service crash and restart while keyboard input is active.
- [ ] R087-T080. Capture exact-SHA G3/G4/G5 normal-boot screenshot and real input evidence.

### Workstream 5: G8 — Kernel reliability, device lifecycle and acceptance
**Source ownership:** `rust/kernel_core/src/interrupts.rs; block.rs; .github/workflows`

- [ ] R087-T081. Run bare-metal no_std compilation for the precise integration SHA.
- [ ] R087-T082. Run host tests covering edited partition and VFS code.
- [ ] R087-T083. Run kernel boot QEMU smoke with the normal default boot path.
- [ ] R087-T084. Verify all eight M1 gate acceptance steps are explicitly represented in CI.
- [ ] R087-T085. Ensure mandatory QEMU tests cannot silently skip without failing the gate.
- [ ] R087-T086. Test unexpected ATA read failure with no dirty caller buffer.
- [ ] R087-T087. Test device capacity change in the middle of selected-volume access.
- [ ] R087-T088. Test simulated disk removal before an attempted partition write.
- [ ] R087-T089. Test driver read timeout and bounded failure propagation.
- [ ] R087-T090. Test keyboard interrupt storms without scheduler starvation.
- [ ] R087-T091. Test malformed pointer interrupts without kernel memory corruption.
- [ ] R087-T092. Test one ring-3 task fault while healthy processes remain runnable.
- [ ] R087-T093. Check abandoned kernel frame reclamation after process crash.
- [ ] R087-T094. Check no unauthorized filesystem write after token revocation.
- [ ] R087-T095. Test normal shutdown waits for durable metadata acknowledgment.
- [ ] R087-T096. Test reboot into explicit recovery state after corrupted boot services.
- [ ] R087-T097. Preserve serial and QEMU exit-code artifacts on failing workflows.
- [ ] R087-T098. Verify CI results on the exact proposed merge commit SHA.
- [ ] R087-T099. Require independent review of memory ownership, driver bounds, and session isolation.
- [ ] R087-T100. Update the 13-gate M1 ledger only after a full real-QEMU acceptance result.

## Current implementation evidence (not gate acceptance)

- Source: `rust/kernel_core/src/block.rs` binds `BlockWindow` to the initially selected parent sector capacity, checks before every bounded read/write and again after successful transfer, staging reads until safe to publish. Detects geometry changes; it does **not** prove device identity when sector count remains unchanged. GPT GUID revalidation remains necessary.
- Tests added: changed parent before I/O (shrink/grow), changed during read (output unchanged), changed during write (uncertain failure) and unchanged-media success.
- Rust compilation and genuine QEMU acceptance **must be confirmed by GitHub Actions**; do not claim they passed from the presence of tests.
- These four tests alone do not close any of the eight M1 gates. The individual tasks remain unchecked pending associated runtime/CI evidence.

## Definition of done

- Explicit guest GUI Terminal command entered with a real QEMU PS/2 keyboard writes a file to a selected, authenticated, validated block-backed VFS.
- A full QEMU shutdown and new boot permits Files/Terminal readback of the same data; recovery/corruption/partition guards pass.
- Normal boot starts independent PID1, Files, Terminal, and user compositor without experimental feature flags.
- Complete mandatory host/no_std, QEMU boot, runtime, persistence and security checks against exact SHA; leave the PR draft if any required result is missing.
- Update `docs/DELIVERY_MILESTONES.md` only when gate evidence closes an item.


## Follow-on implementation within the SAME PR

- **Second kernel source patch:** BlockWindow now permanently invalidates itself after observing a parent capacity mismatch. A disk that shrinks or grows and later returns to its original capacity cannot silently reactivate a previously stale selected window.
- **Two additional fault-injection tests:** stale window remains invalid after capacity restoration; mid-read invalidation stays latched after restoration.
- This is an additional **partial implementation**, not 100 additional completed tasks, not a closed M1 gate. Existing 100 prioritized tasks remain the acceptance scope; do not expand the backlog solely to inflate counts.
- **Required evidence:** GitHub host Rust, no_std, security, and actual QEMU disk persistence checks on the exact new SHA before any acceptance claim.

## 100 more kernel-only G1–G8 tasks

The [second R087 task register](M1_R087_KERNEL_FIRST_200_BATCH.md) adds R087-T101 through R087-T200 in this same PR. The PR now scopes 200 tasks, not 200 implemented features. Source work is partial, and all M1 acceptance gates remain pending.
