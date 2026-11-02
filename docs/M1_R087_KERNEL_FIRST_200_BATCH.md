# IndianOS R087 — 100 more kernel tasks, G1–G8 only (R087-T101–T200)

**State: 100 new engineering work items planned, 0 of these 100 fully accepted.** This complements the existing [first R087 100-task register](M1_R087_KERNEL_FIRST_100_BATCH.md) in **the same PR #736**; 200 tasks scoped, not 200 completed features.

**Priority authority:** [M1 gate tracker #616](https://github.com/pramod07info/indianos/issues/616). Finish the eight remaining high-level gates instead of adding M2/AI/Windows-compatibility/UI theme work.

**Five logical workstreams:** 20 work items each. No claim that five autonomous coding agents are actually running.

**Acceptance rule:** A task requires live production path changes, direct tests, real QEMU guest coverage where relevant, and exact-SHA CI evidence before checking. Code-only helpers and host tests do not close any high-level gate.

## Task register (additional items)

### Workstream 1: G2 / kernel block-device and partition safeguards

- [ ] R087-T101. Pin read-only diagnostic device capacity at the start of each borrow.
- [ ] R087-T102. Reject out-of-range LBA accesses in read-only probes before I/O.
- [ ] R087-T103. Stage diagnostic sector reads before publishing their output.
- [ ] R087-T104. Preserve caller memory when a read-only backend dirties output then fails.
- [ ] R087-T105. Reject diagnostic sector data if capacity changes during the read.
- [ ] R087-T106. Test pinned read-only capacity against unexpected media shrink.
- [ ] R087-T107. Test pinned read-only capacity against unexpected media expansion.
- [ ] R087-T108. Verify read-only diagnostic access still works on stable media.
- [ ] R087-T109. Pin parent capacity across a complete multi-sector read operation.
- [ ] R087-T110. Check parent capacity before every sector in a multi-sector read.
- [ ] R087-T111. Recheck capacity after every successful multi-sector read transfer.
- [ ] R087-T112. Clear every caller output sector if capacity changes mid-read.
- [ ] R087-T113. Preserve original caller output for an out-of-range preflight rejection.
- [ ] R087-T114. Pin parent capacity across a complete multi-sector write operation.
- [ ] R087-T115. Check geometry before each multi-sector write to prevent cross-media writes.
- [ ] R087-T116. Stop multi-sector writes after detecting geometry change post-transfer.
- [ ] R087-T117. Report uncertain status for any partially acknowledged multi-sector write.
- [ ] R087-T118. Verify stable multi-sector writes continue to respect validated boundaries.
- [ ] R087-T119. Recheck GPT disk and partition GUIDs after selected-partition health I/O.
- [ ] R087-T120. Demonstrate real selected-partition QEMU read/write/reboot without host disk writes.

### Workstream 2: G6 / durable filesystem transactions and recovery

- [ ] R087-T121. Define write-through/flush guarantees for the selected ATA storage backend.
- [ ] R087-T122. Verify that acknowledged payload sector writes are durable before metadata commit.
- [ ] R087-T123. Check that COW metadata sectors never publish before all referenced payload writes.
- [ ] R087-T124. Reject truncated COW metadata records in a fresh mount.
- [ ] R087-T125. Verify old COW head remains readable after a failed new-head write.
- [ ] R087-T126. Verify previous valid file bank survives a power interruption.
- [ ] R087-T127. Protect metadata generation increments from wrapping around.
- [ ] R087-T128. Reject a mismatched on-disk COW generation when an old mount tries to commit.
- [ ] R087-T129. Recheck mounted-volume health after an uncertain device write.
- [ ] R087-T130. Revalidate both filesystem superblocks on a new boot.
- [ ] R087-T131. Verify incomplete file rename retains a valid old or new directory structure.
- [ ] R087-T132. Ensure directory deletion cannot strand a live child entry.
- [ ] R087-T133. Check durable file readback using two independent volume mount instances.
- [ ] R087-T134. Keep corrupt metadata probing strictly read-only.
- [ ] R087-T135. Verify quota/capacity failures cannot overwrite a previous good file.
- [ ] R087-T136. Reject stale per-user file ownership on reopened filesystem handles.
- [ ] R087-T137. Inject a write failure during a COW file append.
- [ ] R087-T138. Inject a write failure during a COW directory move.
- [ ] R087-T139. Run a disposable-disk QEMU reboot after an interrupted metadata commit.
- [ ] R087-T140. Publish exact-SHA G6 durability and corruption-recovery artifacts.

### Workstream 3: G1 / G7 live kernel authenticated filesystem ABI

- [ ] R087-T141. Trace the real QEMU keyboard event path into authenticated Terminal command execution.
- [ ] R087-T142. Validate the kernel-resolved caller PID before handling persistent file commands.
- [ ] R087-T143. Validate the process generation on each file descriptor operation.
- [ ] R087-T144. Reject any user-supplied PID that conflicts with the active kernel task.
- [ ] R087-T145. Validate syscall input pointers before copying file paths into kernel memory.
- [ ] R087-T146. Validate syscall output pointers before writing returned file content.
- [ ] R087-T147. Check file-read capability before a selected-partition access.
- [ ] R087-T148. Check file-write capability before any filesystem mutation.
- [ ] R087-T149. Prevent a revoked capability from reaching a cached volume handle.
- [ ] R087-T150. Bind selected GPT partition and disk GUID identity to the active file session.
- [ ] R087-T151. Create a file from an actual ring-3 Terminal command.
- [ ] R087-T152. Read the created file from ring-3 Files using the same durable volume.
- [ ] R087-T153. Edit part of the created file through a bounded authenticated syscall.
- [ ] R087-T154. Rename a file using owner-checked user-mode syscall arguments.
- [ ] R087-T155. Close all file handles when an application task exits.
- [ ] R087-T156. Release file locks after a fatal user process exception.
- [ ] R087-T157. Prevent stale PID-generation handles from accessing rebooted user sessions.
- [ ] R087-T158. Test a second user process failing to mutate the first process's protected file.
- [ ] R087-T159. Run a real guest file write, reboot, and independently mounted GUI readback.
- [ ] R087-T160. Capture exact-SHA G1/G7 guest input and syscall traces.

### Workstream 4: G3 / G4 / G5 normal boot, ring-3 compositor and real input

- [ ] R087-T161. Verify default boot initializes the user process loader without opt-in test flags.
- [ ] R087-T162. Validate the PID1 ELF image before setting up its user page table.
- [ ] R087-T163. Launch PID1 with its own protected user stack and guard page.
- [ ] R087-T164. Launch Terminal as an independent ring-3 task.
- [ ] R087-T165. Launch Files as an independent ring-3 task.
- [ ] R087-T166. Launch a compositor as a ring-3 service rather than relying on kernel window policy.
- [ ] R087-T167. Enforce per-process ownership of graphical surface mappings.
- [ ] R087-T168. Reject a compositor client write outside its allocated surface.
- [ ] R087-T169. Verify the compositor survives a blocked storage client.
- [ ] R087-T170. Deliver real QEMU PS/2 keyboard make and break events into the input broker.
- [ ] R087-T171. Translate modifier state without accepting malformed scancodes.
- [ ] R087-T172. Prevent stuck key state after a keyboard controller error.
- [ ] R087-T173. Route real pointer motion to the focused graphical client.
- [ ] R087-T174. Reject malformed mouse packet lengths and invalid screen coordinates.
- [ ] R087-T175. Release pointer capture on client process exit.
- [ ] R087-T176. Release keyboard focus after the focused app crashes.
- [ ] R087-T177. Show editable Terminal command text in the QEMU framebuffer.
- [ ] R087-T178. Verify a real mouse click can open and select a file in Files.
- [ ] R087-T179. Restart a crashed desktop service without reusing stale session authority.
- [ ] R087-T180. Store a genuine normal-boot screenshot and process-ID evidence for G3–G5.

### Workstream 5: G8 kernel reliability, hardware drivers and acceptance

- [ ] R087-T181. Compile kernel-core Rust host tests on the proposed commit SHA.
- [ ] R087-T182. Compile freestanding x86-64 no_std kernel code for the proposed SHA.
- [ ] R087-T183. Run the normal kernel QEMU/UEFI boot smoke test.
- [ ] R087-T184. Run a disposable-disk persistent-files GUI smoke test.
- [ ] R087-T185. Run the selected-partition GPT corruption rejection suite.
- [ ] R087-T186. Run malformed protective MBR rejection scenarios without writes.
- [ ] R087-T187. Inject a backend that partially fills its read buffer before failing.
- [ ] R087-T188. Inject a device that changes capacity during a sector read.
- [ ] R087-T189. Inject a device that changes capacity during a multi-sector write.
- [ ] R087-T190. Verify no subsequent backend writes after detecting a mid-transfer media change.
- [ ] R087-T191. Verify stable media still completes ordinary read/write transactions.
- [ ] R087-T192. Inject an ATA busy timeout and verify a bounded recovery path.
- [ ] R087-T193. Verify kernel serial logging survives a missing disk.
- [ ] R087-T194. Verify kernel serial logging survives a missing PS/2 controller.
- [ ] R087-T195. Run ring-3 address-space isolation adversarial tests.
- [ ] R087-T196. Verify file access tokens do not survive PID recycling.
- [ ] R087-T197. Run a two-boot durable VFS readback with real QEMU media.
- [ ] R087-T198. Run normal graphical keyboard and mouse acceptance through QEMU.
- [ ] R087-T199. Attach exact-SHA regression logs, screenshots and QEMU exit statuses to PR.
- [ ] R087-T200. Close an M1 gate only after its complete end-to-end exit criteria pass.

## Implemented source delta (partial, not accepted gates)

- `rust/kernel_core/src/block.rs` now pins read-only probe capacity and stages diagnostic reads; backend failure and mid-read geometry changes cannot dirty the caller's buffer.
- Shared `read_sectors` and `write_sectors` now pin and check capacity before/after individual transfers. Multi-sector read on change zeroes the complete caller output. Multi-sector write stops on uncertain geometry to avoid subsequent writes to a different-sized device.
- Six added unit tests cover dirty output on read failure, pre-read size changes, post-read size changes, multi-read clearing, multi-write stopping and stable media compatibility.
- Earlier R087 source changes also permanently revoke `BlockWindow` after it observes changed parent capacity. That does not detect equal-capacity disk replacement; GPT GUID checks remain required.
- **0 / 8 new M1 gates closed** by this source delta alone. A real QEMU ring-3 keyboard-to-file-to-reboot readback is still needed.

## Gate exit rules

- All required host, no_std, kernel boot, persistent-media and QEMU guest input checks must pass on the exact final SHA; attach evidence and keep PR draft if anything is missing.
- Do not mark tasks complete because a source patch or test exists; reconcile each task against behavior, tests and exact-sha evidence. Do not issue a release or perform physical disk writes.

## Additional G2 production slice — partition discovery

- `rust/kernel_core/src/m1_partition.rs`: discovery now stages a raw sector before publishing it and explicitly zeroes caller output on driver I/O failure or a detected capacity change. Prior capacity/bounds checks remain in place.
- Two regression tests exercise a driver that dirties its buffer and fails, and stable read-only MBR discovery with no writes.
- This is source-level hardening only, not evidence of a completed G2 QEMU mount/reboot acceptance gate.
