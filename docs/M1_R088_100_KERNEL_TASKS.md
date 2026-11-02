# IndianOS R088 — 100 kernel engineering tasks / 5 logical workstreams

**Branch:** `feature/m1-r088-five-stream-kernel-100`
**Base main:** `750b1e10fed2692baf13d3412a36d80d4ce6d75e` (PR #736 is merged).
**Priority:** Close M1's eight remaining outcome gates [#616](https://github.com/pramod07info/indianos/issues/616). No M2, AI, Windows/macOS compatibility, UI theming, or superficial agent placeholder files.

**Truthful status:** 100 clearly scoped acceptance tasks, **not 100 completed features**. This batch contains real production changes in five kernel subsystems and fault-injection tests; tasks remain unchecked until source, wiring, tests, QEMU acceptance and exact-SHA evidence demonstrate the individual outcome. Five workstreams are ownership slices, **not a claim that five independent coding agents/workstations are running**.

**Integration policy:** single consolidated implementation commit and one draft PR. Never merge before mandatory host Rust, freestanding no_std, QEMU disk/boot, PS/2 and security checks pass. Host-only tests never close a ring-3 or physical-input gate. Physical disk writes prohibited.

## Workstream task register (R088-T001–T100)

### W1 — Kernel block/extent safety (G2, G6, G7)
**Owned production file:** `rust/kernel_core/src/block.rs`

- [ ] R088-T001. Invalidate read-only storage probes permanently after observing parent capacity changes.
- [ ] R088-T002. Stage diagnostic single-sector reads so driver failures cannot dirty caller buffers.
- [ ] R088-T003. Reject a previously invalidated read-only probe even after capacity restoration.
- [ ] R088-T004. Cover mid-transfer device geometry changes with a separate negative regression.
- [ ] R088-T005. Preserve hard read-only capability even when a probe enters the invalid state.
- [ ] R088-T006. Pin extent-reader parent capacity at successful extent-map validation.
- [ ] R088-T007. Stop extent reads when physical capacity changes after validating extents.
- [ ] R088-T008. Revoke an extent reader when geometry changes and then returns to its old value.
- [ ] R088-T009. Stage mapped sector contents until physical read and capacity postcheck succeed.
- [ ] R088-T010. Preserve caller sector buffer when an extent backend fails after writing bytes.
- [ ] R088-T011. Clear a multi-extent byte-output buffer if a later physical transfer fails.
- [ ] R088-T012. Preserve caller bytes on a purely preflight extent-bounds rejection.
- [ ] R088-T013. Test the extent mapping of discontiguous sectors after geometry hardening.
- [ ] R088-T014. Test multiple extent boundaries using the real mapped BlockDevice interface.
- [ ] R088-T015. Reject overlap and overflow in extent validation before any device access.
- [ ] R088-T016. Propagate backend media-change errors to the caller without reusing stale sectors.
- [ ] R088-T017. Verify mounted selected-partition reads never cross their physical window.
- [ ] R088-T018. Run a genuine QEMU selected-volume block read after IDE recovery.
- [ ] R088-T019. Run a QEMU selected-volume block write and remount without altering neighboring partitions.
- [ ] R088-T020. Record the exact-SHA G2/G6/G7 read/write evidence and unresolved device identity limits.

### W2 — Partition discovery and GPT identity (G2, G1)
**Owned production file:** `rust/kernel_core/src/m1_partition.rs`

- [ ] R088-T021. Bind a read-only partition discovery session to the initially selected device geometry.
- [ ] R088-T022. Latch discovery invalidation after any observed parent-device capacity change.
- [ ] R088-T023. Reject a discovery transfer after capacity returns to its original value.
- [ ] R088-T024. Keep the discovery byte buffer unchanged for pre-read rejected device changes.
- [ ] R088-T025. Scrub discovery output when a device changes size after a completed transfer.
- [ ] R088-T026. Test capacity change before the first MBR read without issuing backend I/O.
- [ ] R088-T027. Test capacity change during the protective-MBR read without issuing writes.
- [ ] R088-T028. Keep unchanged MBR discovery compatible with the latching adapter.
- [ ] R088-T029. Validate GPT primary header checksum before trusting partition bounds.
- [ ] R088-T030. Validate GPT secondary header and entry-array consistency before mount selection.
- [ ] R088-T031. Reject duplicate unique live partition GUIDs even when partitions do not overlap.
- [ ] R088-T032. Reject GPT entry LBA overlaps and allocations crossing GPT metadata.
- [ ] R088-T033. Reject a hybrid protective MBR instead of choosing a physical volume automatically.
- [ ] R088-T034. Preserve sparse partition ordinals in a complete, validated partition catalog.
- [ ] R088-T035. Reject a reused partition token after disk-GUID rotation.
- [ ] R088-T036. Reject a reused partition token after unique partition-GUID rotation.
- [ ] R088-T037. Recheck selected GPT identity after a real partition-backed VFS health probe.
- [ ] R088-T038. Ensure invalid partition discovery cannot autoformat any device.
- [ ] R088-T039. Run selected GPT QEMU mount with legitimate filesystem content across reboot.
- [ ] R088-T040. Record exact-SHA G2/G1 GUI Terminal mount and corrupted-media acceptance artifacts.

### W3 — COW durable filesystem and read integrity (G6, G1, G7)
**Owned production file:** `rust/kernel_core/src/durablefs.rs`

- [ ] R088-T041. Pin device capacity throughout COW payload checksum verification.
- [ ] R088-T042. Reject data-bank verification on a parent geometry change between sectors.
- [ ] R088-T043. Reject a capacity change occurring during a successful payload-sector read.
- [ ] R088-T044. Recheck mounted filesystem geometry before exposing file bytes to a caller.
- [ ] R088-T045. Recheck mounted filesystem geometry after every file data-sector transfer.
- [ ] R088-T046. Keep the user-visible read buffer untouched on a detected geometry change.
- [ ] R088-T047. Test a COW mount whose payload read changes media geometry mid-verification.
- [ ] R088-T048. Test an existing mount whose file data read changes parent media capacity.
- [ ] R088-T049. Preserve the previous checksum-valid COW generation after an interrupted overwrite.
- [ ] R088-T050. Reject same-generation conflicting superblock heads before selecting a checkpoint.
- [ ] R088-T051. Reject divergent or non-adjacent valid superblock generation numbers.
- [ ] R088-T052. Validate the full stored file checksum even for a partial file read.
- [ ] R088-T053. Verify directory renames publish one atomic metadata generation.
- [ ] R088-T054. Verify failed file creates do not reassign banks held by older generations.
- [ ] R088-T055. Require a live authenticated owner for an active filesystem mutation path.
- [ ] R088-T056. Require explicit recovery authorization before repairing a degraded volume.
- [ ] R088-T057. Inject failed ATA payload writes before COW metadata publication.
- [ ] R088-T058. Inject failed metadata-sector writes and prove the previous commit remains mountable.
- [ ] R088-T059. Run guest QEMU file creation and readback across two separate boots.
- [ ] R088-T060. Capture exact-SHA G6/G1/G7 recovery, ownership and power-loss acceptance results.

### W4 — ATA PIO driver transaction and recovery (G8, G2, G6)
**Owned production file:** `rust/kernel_core/src/ata_pio.rs`

- [ ] R088-T061. Stage a complete 512-byte ATA sector in private kernel memory before publishing.
- [ ] R088-T062. Publish ATA read results only if the final controller completion check succeeds.
- [ ] R088-T063. Retain caller bytes if a controller reports error after transferring all PIO words.
- [ ] R088-T064. Keep a finite polling bound for all ATA readiness and flush phases.
- [ ] R088-T065. Preserve ATA failure-stage diagnostics for final DRQ/BSY completion errors.
- [ ] R088-T066. Test an ATA read that fills all 256 words but fails the completion check.
- [ ] R088-T067. Test successful full-sector ATA reads with little-endian word conversion.
- [ ] R088-T068. Reject malformed ATA IDENTIFY data missing required LBA support.
- [ ] R088-T069. Reject unsupported ATA 4Kn logical-sector configurations.
- [ ] R088-T070. Reject LBA28 range overflow before programming task-file registers.
- [ ] R088-T071. Keep ATA write completion ordered before the FLUSH CACHE command.
- [ ] R088-T072. Reject error/DF status before exposing or writing another sector.
- [ ] R088-T073. Exercise a missing ATA disk on the QEMU primary controller.
- [ ] R088-T074. Exercise a slow ATA drive within the bounded polling policy.
- [ ] R088-T075. Exercise a timeout after the PIO data phase without publishing caller output.
- [ ] R088-T076. Verify storage failure diagnostics identify the exact failed transfer stage.
- [ ] R088-T077. Verify no read or write can reach an unauthorized physical disk.
- [ ] R088-T078. Run a QEMU disposable IDE read → write → FLUSH → readback integration test.
- [ ] R088-T079. Run a QEMU controlled shutdown/reboot to verify acknowledged disk writes persist.
- [ ] R088-T080. Attach exact-SHA G8/G2/G6 kernel boot/ATA diagnostic and recovery evidence.

### W5 — PS/2 pointing device and ring-3 input boundary (G5, G4, G3)
**Owned production file:** `rust/kernel_core/src/ps2_mouse.rs`

- [ ] R088-T081. Reject PS/2 X-axis motion bytes inconsistent with packet sign flags.
- [ ] R088-T082. Reject PS/2 Y-axis motion bytes inconsistent with packet sign flags.
- [ ] R088-T083. Reject malformed pointing-device frames before updating button state.
- [ ] R088-T084. Clear consumed mouse packet bytes before decoding the next packet.
- [ ] R088-T085. Resynchronize to the next valid packet after an overflow frame.
- [ ] R088-T086. Test invalid sign bits cannot forge a left-button click.
- [ ] R088-T087. Test valid negative X/Y motion still decodes correctly.
- [ ] R088-T088. Test overflow frames do not preserve stale packet contents.
- [ ] R088-T089. Keep primary keyboard bytes outside the auxiliary mouse read path.
- [ ] R088-T090. Reject parity/timeout mouse bytes before packet decode.
- [ ] R088-T091. Enforce a finite retry bound for AUX command acknowledgment.
- [ ] R088-T092. Handle invalid AUX acknowledgment without stealing keyboard interrupts.
- [ ] R088-T093. Ensure mouse button transitions follow an accepted complete packet only.
- [ ] R088-T094. Bound mouse cursor deltas before forwarding events to an app surface.
- [ ] R088-T095. Enforce focused window ownership on pointer dispatch.
- [ ] R088-T096. Revoke pointer capture when the owner PID-generation exits.
- [ ] R088-T097. Keep the input broker independent from the privileged compositor backend.
- [ ] R088-T098. Run QEMU real mouse motion through a focus change.
- [ ] R088-T099. Run QEMU real mouse click on Files/Terminal and capture the framebuffer.
- [ ] R088-T100. Attach exact-SHA G5/G4/G3 PS/2 and ring-3 input isolation evidence.

## Implemented source slice in this batch

1. **Block and mapped extents:** read-only device sessions latch geometry invalidation, extent readers pin the mapped parent and stage sectors, and multi-extent byte reads zero the whole output on in-flight I/O failure.
2. **Partition discovery:** a media-size change permanently invalidates the current diagnostic session; no formatting or partition writing is enabled.
3. **Durable COW filesystem:** checks media geometry during payload verification and at every mounted-file read before exposing bytes.
4. **QEMU ATA PIO:** a physical sector is staged until all words and the completion-status check pass; the real driver uses this function.
5. **PS/2 mouse:** rejects malformed sign/motion combinations and scrubs frame bytes without issuing a button event.

**New host regression tests:** 5 block/extent + 2 partition + 2 durablefs + 2 ATA + 2 mouse = **13** (subject to exact-SHA compilation and CI).

## Mandatory acceptance before merging

- No new high-level gate is closed by the presence of these source changes alone. The 5/13 high-level M1 baseline remains until verified guest interactions, reboot/remount and owner isolation.
- Check source tests and freestanding no_std compilation, plus exact-SHA kernel boot, durable disk and physical input QEMU runs; resolve failures in code rather than skipping tests.
- Prioritize G1/G3/G4/G7 normal ring-3 desktop integration and full guest end-to-end proof after these lower-level safety changes. A keyboard-to-durable-file-to-reboot readback is still required.
- Only one integration commit and one PR. Report which tasks are actually implemented, which are tested, which are unverified, and which gates remain open.
