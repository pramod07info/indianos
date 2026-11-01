# IndianOS R085 — 100-task integration backlog / five logical workstreams

Base: merged R084 PR #729, main SHA `8d5419979ae8bafee8994ca482bfce2c9845eb2f`.

## Source delivery versus the original 100-task request

100 acceptance-oriented tasks were allocated across five mutually scoped kernel workstreams. **Only 26 source-level tasks are implemented by this initial commit; 74 remain pending.** A checked box means the relevant Rust source and targeted host tests were added, **not** that GitHub CI or QEMU has passed. Five logical ownership scopes are not five autonomous coding agents.

These changes are deliberately integrated with five existing production modules rather than generating placeholder `agent_XX.rs` files. Outstanding tasks requiring live boot, storage, process isolation, networking, or actual hardware evidence stay unchecked.

## Numbered acceptance checklist

### A. Boot-service and PID1 start safety

- [x] 001. Validate entire service dependency graph before startup
- [x] 002. Offer validated next-service selection
- [x] 003. Register service manifests atomically
- [x] 004. Confirm readiness transitions as one atomic ordered batch
- [x] 005. Count essential services still unready
- [x] 006. Query exact service readiness
- [ ] 007. Wire validated plan into real PID1 boot path
- [ ] 008. Make missing dependency an explicit boot refusal
- [ ] 009. Make service cycle an explicit boot refusal
- [ ] 010. Bind ready transitions to authenticated process identity
- [ ] 011. Enforce service startup deadline
- [ ] 012. Make essential-service failure fail closed
- [ ] 013. Add real child process launch and status integration
- [ ] 014. Prevent duplicate startup after scheduler retry
- [ ] 015. Transfer capabilities only after readiness confirmation
- [ ] 016. Report service dependency errors through QEMU serial diagnostics
- [ ] 017. Test actual PID1-to-supervisor restart handoff
- [ ] 018. Persist explicit service failure reason for recovery
- [ ] 019. Prove headless-first startup in QEMU
- [ ] 020. Prove normal desktop boot with dependency ordering

### B. Kernel IPC event integrity

- [x] 021. Count queued events
- [x] 022. Count exact-recipient-and-generation events
- [x] 023. Publish authenticated event batch transactionally
- [x] 024. Receive exact-owner event group with short-buffer rollback
- [x] 025. Allow exact-recipient event-kind cancellation
- [ ] 026. Wire batch API into actual syscall dispatcher
- [ ] 027. Authenticate sender PID from scheduler PCB
- [ ] 028. Bind sender process generation into event envelope
- [ ] 029. Reject stale sender generation after PID recycling
- [ ] 030. Revoke sender and receiver events after task exit
- [ ] 031. Implement bounded wakeup notification for blocked consumer
- [ ] 032. Handle receiver mailbox overflow without interrupt loss
- [ ] 033. Define fair scheduling between event recipients
- [ ] 034. Add IPC timeout and cancellation
- [ ] 035. Provide capability-checked cross-service channel creation
- [ ] 036. Audit IPC sequence ordering under interleaved processes
- [ ] 037. Prove ring-3 request/response isolation in QEMU
- [ ] 038. Prove mailbox recovery after process restart
- [ ] 039. Stress fixed-capacity FIFO under timer preemption
- [ ] 040. Prove no kernel memory disclosure in event output

### C. Physical keyboard and input security

- [x] 041. Expose total pending key-event count
- [x] 042. Expose exact PID-generation key-event count
- [x] 043. Inspect trusted focused window lease
- [x] 044. Clear focus only for matching exact lease
- [x] 045. Remove events targeting closed/recycled windows
- [x] 046. Receive only live-window keyboard events
- [x] 047. Publish physical-key input group atomically
- [ ] 048. Wire live receive into real input syscall service
- [ ] 049. Authenticate device event source at PS/2 input boundary
- [ ] 050. Prevent stale process focus after crash-restart
- [ ] 051. Handle key repeat safely with bounded policy
- [ ] 052. Define modifier state per authenticated session
- [ ] 053. Deliver key release without stuck modifiers
- [ ] 054. Ensure alt-tab focus does not leak queued keys
- [ ] 055. Enforce input buffer backpressure in QEMU
- [ ] 056. Test real PS/2 mouse/keyboard concurrency
- [ ] 057. Add keyboard-only window accessibility acceptance
- [ ] 058. Add input event diagnostics to crash reporter
- [ ] 059. Prove cross-PID keyboard isolation in QEMU
- [ ] 060. Prove focus recovery after compositor restart

### D. Window compositor ownership and geometry

- [x] 061. Create multiple windows atomically
- [x] 062. Close a window only for exact PID generation owner
- [x] 063. Update window geometry with optimistic compare-and-swap
- [x] 064. Count per-owner windows
- [x] 065. Perform bounded trusted compositor hit testing
- [ ] 066. Wire compositor hit testing to real PS/2 mouse input
- [ ] 067. Move full compositor logic to isolated ring-3 process
- [ ] 068. Map per-client surface memory with kernel isolation
- [ ] 069. Establish versioned frame-delivery syscall/IPC ABI
- [ ] 070. Reclaim surfaces during process teardown
- [ ] 071. Rotate ownership correctly on PID reuse
- [ ] 072. Implement bounded damage-to-frame acknowledgement
- [ ] 073. Enforce clipping on actual user-provided draw buffers
- [ ] 074. Prove multiwindow focus across compositor restarts
- [ ] 075. Implement secure keyboard focus and stacking order
- [ ] 076. Add HiDPI coordinate transforms to real rendering path
- [ ] 077. Test framebuffer fallback after resize failure
- [ ] 078. Prove 1280x800 QEMU window screenshots at current SHA
- [ ] 079. Exercise two independent user ELFs with real pointer movement
- [ ] 080. Verify no ring-3 access to other windows' frame data

### E. Durable file access and security gates

- [x] 081. Enforce protected namespaces on authenticated guest-file replacements
- [x] 082. Add authenticated capability-gated read wrapper
- [x] 083. Add bounded readback integrity verification after replacement
- [ ] 084. Integrate write guard into all privileged VFS mutation paths
- [ ] 085. Integrate exact file-handle quotas into durable HierVfs
- [ ] 086. Propagate canonical-path denials to userland error UI
- [ ] 087. Add per-process and per-directory quota accounting
- [ ] 088. Persist global protection state through power-cut replay
- [ ] 089. Add actual checksummed copy-on-write metadata journal
- [ ] 090. Enforce atomic rename with recovery boundary
- [ ] 091. Prove durable multi-file transaction across QEMU reboot
- [ ] 092. Prove fsync/flush ordering with fault injection
- [ ] 093. Protect system recovery partitions from guest mutation
- [ ] 094. Require signer trust for system path updates
- [ ] 095. Recover from truncated metadata mirror
- [ ] 096. Validate disk UUID/partition bounds at mount
- [ ] 097. Ensure runtime volumes survive five repeated QEMU boots
- [ ] 098. Implement authenticated user-namespace ACL on actual disk reads
- [ ] 099. Prove guest ring-3 cannot bypass protected paths
- [ ] 100. Add corruption/recovery acceptance evidence to CI

## Source changes introduced

- `rust/kernel_core/src/batch2_init.rs`: full graph validation; atomic service registration and readiness publication; essential completion statistics; negative cycle/dependency tests.
- `rust/kernel_core/src/batch2_events.rs`: per-owner counts, exact-owner atomic multi-message send and receive, owner-kind cancellation, rollback and leakage tests.
- `rust/kernel_core/src/batch2_keyboard.rs`: stale/retired window event purging, live receive, authenticated focus inspection, compare-clear, atomic physical-key batches.
- `rust/kernel_core/src/batch2_windows.rs`: atomic window creation, generation-pinned close, optimistic geometry updates, bounded hit-test and owner counts.
- `rust/kernel_core/src/batch6_storage.rs`: `WriteGuard` protected-name policy on guest writes, auth-gated read API and read-after-write check. This protects this API route but is **not** a global durable VFS ACL.

## Validation required before merge

- Rust host unit tests and `no_std` cross-compilation at the exact PR head SHA.
- Existing combined regression, adversarial security, durable Files/Terminal, QEMU runtime, and native kernel ELF/ISO boot workflows.
- Review branches and CI coordination check. Fix real source errors and amend/re-squash this same single source commit; do not skip tests.

Do not claim all 100 tasks are complete: API-level code is not evidence of ring-3 compositor functionality, real persistent storage, independent agent execution, or a production kernel.


## R085 extension — another 100 source operations in the SAME pull request

**Expansion scope:** 100 additional bounded Rust code-level operations (#101–200), 20 per existing kernel subsystem. These do not replace the first 100 planned acceptance tasks: of the initial 100, only **26** have source implementation, and **74 high-level tasks are still open**. The new 100 operations make **126 source-level implemented items across a 200-item mixed-granularity checklist**, not 126 completed OS acceptance gates. Operations are real code in existing modules; many are introspection, per-owner queue selection and transactional policies, not independently runnable OS features. Five *logical* ownership scopes; **no five autonomous agent processes were spawned**.

**Code and targeted unit tests are written in the amended integration commit; fresh full CI on the amended SHA is mandatory**. Do not infer successful QEMU boot, complete ring-3 graphics, disk persistence, global filesystem ACL, or hardware suitability from these API additions.

### Boot service graph and PID1 admission — 20 operations

Source: `rust/kernel_core/src/batch2_init.rs`. API-level source implementation and adversarial Rust tests; not boot integration.

- [x] 101. `registered_count`
- [x] 102. `available_slots`
- [x] 103. `ready_count`
- [x] 104. `pending_count`
- [x] 105. `essential_count`
- [x] 106. `essential_ready_count`
- [x] 107. `optional_pending_count`
- [x] 108. `first_unready`
- [x] 109. `last_unready`
- [x] 110. `first_ready`
- [x] 111. `get_service`
- [x] 112. `has_service`
- [x] 113. `dependency_ready`
- [x] 114. `ready_service_ids`
- [x] 115. `unready_service_ids`
- [x] 116. `essential_service_ids`
- [x] 117. `pending_dependents`
- [x] 118. `dependency_depth`
- [x] 119. `maximum_dependency_depth`
- [x] 120. `audit_ready_dependencies`

### Authenticated IPC mailbox — 20 operations

Source: `rust/kernel_core/src/batch2_events.rs`. API-level source implementation and adversarial Rust tests; not boot integration.

- [x] 121. `available_slots`
- [x] 122. `is_empty`
- [x] 123. `is_full`
- [x] 124. `count_sender`
- [x] 125. `count_kind_for`
- [x] 126. `count_from_for`
- [x] 127. `count_sender_kind`
- [x] 128. `first_for`
- [x] 129. `last_for`
- [x] 130. `first_kind_for`
- [x] 131. `last_kind_for`
- [x] 132. `peek_at_for`
- [x] 133. `snapshot_for`
- [x] 134. `snapshot_kind_for`
- [x] 135. `receive_kind_for`
- [x] 136. `receive_from_for`
- [x] 137. `cancel_all_for`
- [x] 138. `cancel_from_for`
- [x] 139. `cancel_kinds_for`
- [x] 140. `drop_sender`

### Keyboard and focus isolation — 20 operations

Source: `rust/kernel_core/src/batch2_keyboard.rs`. API-level source implementation and adversarial Rust tests; not boot integration.

- [x] 141. `queue_capacity`
- [x] 142. `remaining_slots`
- [x] 143. `queue_empty`
- [x] 144. `queue_full`
- [x] 145. `is_focused_exact`
- [x] 146. `focused_window`
- [x] 147. `pending_window`
- [x] 148. `pending_scan`
- [x] 149. `pending_pressed`
- [x] 150. `pending_released`
- [x] 151. `peek_owner`
- [x] 152. `peek_last_owner`
- [x] 153. `snapshot_owner`
- [x] 154. `receive_window`
- [x] 155. `receive_scan`
- [x] 156. `cancel_window`
- [x] 157. `cancel_scan`
- [x] 158. `discard_owner`
- [x] 159. `clear_all_input`
- [x] 160. `pending_live`

### Window surface ownership and atomic geometry — 20 operations

Source: `rust/kernel_core/src/batch2_windows.rs`. API-level source implementation and adversarial Rust tests; not boot integration.

- [x] 161. `window_count`
- [x] 162. `free_slots`
- [x] 163. `is_empty`
- [x] 164. `is_full`
- [x] 165. `first_window`
- [x] 166. `last_window`
- [x] 167. `first_owner_window`
- [x] 168. `last_owner_window`
- [x] 169. `owner_window_at`
- [x] 170. `snapshot_owner`
- [x] 171. `close_batch_exact`
- [x] 172. `move_batch_cas`
- [x] 173. `translate_exact`
- [x] 174. `resize_exact`
- [x] 175. `move_centered`
- [x] 176. `snap_left`
- [x] 177. `snap_right`
- [x] 178. `overlap_count`
- [x] 179. `hit_test_owner`
- [x] 180. `validate_state`

### RAM filesystem integrity and exact-revision edits — 20 operations

Source: `rust/kernel_core/src/batch7_ramfs.rs`. API-level source implementation and adversarial Rust tests; not boot integration.

- [x] 181. `file_capacity`
- [x] 182. `free_file_slots`
- [x] 183. `is_empty_volume`
- [x] 184. `contains_path`
- [x] 185. `length_of`
- [x] 186. `total_bytes`
- [x] 187. `remaining_content_capacity`
- [x] 188. `largest_content`
- [x] 189. `smallest_content`
- [x] 190. `count_empty_files`
- [x] 191. `count_nonempty_files`
- [x] 192. `count_in_directory`
- [x] 193. `bytes_in_directory`
- [x] 194. `count_with_suffix`
- [x] 195. `check_invariants`
- [x] 196. `purge_empty_if_revision`
- [x] 197. `replace_with_zeros_if_file_revision`
- [x] 198. `swap_bytes_if_file_revision`
- [x] 199. `zero_range_if_file_revision`
- [x] 200. `invert_ascii_case_if_file_revision`

### Additional acceptance limitations

- Boot graph introspection does not start or authenticate PID1 services; actual scheduler/loader orchestration remains open.
- IPC and keyboard identity and generation are supplied by trusted kernel ingress. No claim of new ring-3 syscall wiring or physical PS/2 input acceptance in this extension.
- Window hit-testing is policy-only and uses registration order, **not** a ring-3 compositor or a live pixel/frame transport.
- RAM filesystem methods are intentionally volatile; they do not make durable QEMU disk contents or recovery transactions.
- Existing R085 guest-file storage protected namespace checks remain scoped to that route and are not a global filesystem ACL.
- Fresh host regression, no_std, adversarial security, QEMU runtime, and ELF/ISO boot checks must all succeed on this exact new source SHA before merge.
- Keep one source commit and the existing PR #732. Never count independently spawned agents, passed tests or completed OS gates without evidence.


## R085 extension III — 100 more code-level operations in original PR #732

This is the **third 100-operation scope**, numbered **201–300**. Five existing production modules received 20 additional Rust APIs each plus explicit tests for state mutation, owner/generation isolation, invalid input, output capacity, atomic rollback and stale revisions. There are now **200 appended code-level operations (#101–300)** beyond the originally requested 100 acceptance items (#001–100). The original acceptance items remain only **26 source-implemented / 74 pending**; across the mixed 300-item checklist, **226** have source implementations and **74** original acceptance tasks remain open. **This is NOT 226 fully verified OS features or 226 closed kernel gates.** Five workstreams are logical ownership, not five independent spawned coding agents.

The source changes are appended to the same one-source-commit integration branch. No second PR, duplicate agent files, or bypass of mandatory checks. Host tests, no_std/security, QEMU runtime and native ELF/ISO serial boot must be checked at the *new SHA*; until then CI status is **unverified**.

### Kernel service dependency planning (20 new source operations)

Source: `rust/kernel_core/src/batch2_init.rs`. These methods do not by themselves implement physical hardware, disk durability, or ring-3 syscall integration.

- [x] 201. `root_count`
- [x] 202. `dependent_count`
- [x] 203. `root_ids`
- [x] 204. `dependent_ids`
- [x] 205. `leaf_count`
- [x] 206. `leaf_ids`
- [x] 207. `service_essential`
- [x] 208. `service_pending`
- [x] 209. `all_essential_ready`
- [x] 210. `all_registered_ready`
- [x] 211. `parent_id`
- [x] 212. `parent_chain`
- [x] 213. `ancestor_of`
- [x] 214. `ready_direct_dependents`
- [x] 215. `pending_roots`
- [x] 216. `next_ready_essential`
- [x] 217. `next_ready_optional`
- [x] 218. `schedulable_count`
- [x] 219. `schedulable_ids`
- [x] 220. `topological_ids`

### Exact-owner IPC event delivery (20 new source operations)

Source: `rust/kernel_core/src/batch2_events.rs`. These methods do not by themselves implement physical hardware, disk durability, or ring-3 syscall integration.

- [x] 221. `can_enqueue`
- [x] 222. `owner_has_kind`
- [x] 223. `sender_has_kind`
- [x] 224. `pending_from_kind_for`
- [x] 225. `pending_payload_bytes_for`
- [x] 226. `total_payload_bytes`
- [x] 227. `first_from_for`
- [x] 228. `last_from_for`
- [x] 229. `first_matching_for`
- [x] 230. `receive_matching_for`
- [x] 231. `receive_up_to_for`
- [x] 232. `receive_kind_up_to_for`
- [x] 233. `send_if_absent`
- [x] 234. `replace_first_exact`
- [x] 235. `cancel_matching_for`
- [x] 236. `cancel_first_kind_for`
- [x] 237. `discard_first_for`
- [x] 238. `retain_last_for`
- [x] 239. `retain_first_for`
- [x] 240. `retire_kind`

### Physical keyboard queue and focus isolation (20 new source operations)

Source: `rust/kernel_core/src/batch2_keyboard.rs`. These methods do not by themselves implement physical hardware, disk durability, or ring-3 syscall integration.

- [x] 241. `can_queue`
- [x] 242. `owner_has_pending`
- [x] 243. `focused_live`
- [x] 244. `stale_count`
- [x] 245. `first_window_event`
- [x] 246. `last_window_event`
- [x] 247. `scan_pressed_count`
- [x] 248. `scan_released_count`
- [x] 249. `snapshot_window`
- [x] 250. `receive_up_to`
- [x] 251. `receive_window_up_to`
- [x] 252. `cancel_pressed`
- [x] 253. `cancel_released`
- [x] 254. `cancel_scans`
- [x] 255. `switch_focus_drop_pending`
- [x] 256. `physical_if_not_queued`
- [x] 257. `physical_unique_batch`
- [x] 258. `receive_live_up_to`
- [x] 259. `owner_first_scan`
- [x] 260. `owner_last_scan`

### Window ownership, static z-order and layout (20 new source operations)

Source: `rust/kernel_core/src/batch2_windows.rs`. These methods do not by themselves implement physical hardware, disk durability, or ring-3 syscall integration.

- [x] 261. `area_of`
- [x] 262. `owner_area`
- [x] 263. `owner_has_window`
- [x] 264. `window_at`
- [x] 265. `snapshot_all`
- [x] 266. `window_contains`
- [x] 267. `windows_overlap`
- [x] 268. `intersection`
- [x] 269. `count_at`
- [x] 270. `owner_count_at`
- [x] 271. `raise_exact`
- [x] 272. `lower_exact`
- [x] 273. `raise_batch_exact`
- [x] 274. `close_if_rect`
- [x] 275. `swap_geometry`
- [x] 276. `tile_top_bottom`
- [x] 277. `snap_top`
- [x] 278. `snap_bottom`
- [x] 279. `move_clamped`
- [x] 280. `union_bounds`

### Byte-accurate revision-pinned RAM filesystem (20 new source operations)

Source: `rust/kernel_core/src/batch7_ramfs.rs`. These methods do not by themselves implement physical hardware, disk durability, or ring-3 syscall integration.

- [x] 281. `read_exact_if_file_revision`
- [x] 282. `read_range_if_file_revision`
- [x] 283. `read_prefix_if_file_revision`
- [x] 284. `read_suffix_if_file_revision`
- [x] 285. `byte_at_if_file_revision`
- [x] 286. `first_byte_if_file_revision`
- [x] 287. `last_byte_if_file_revision`
- [x] 288. `count_byte_if_file_revision`
- [x] 289. `find_literal_if_file_revision`
- [x] 290. `find_last_literal_if_file_revision`
- [x] 291. `starts_with_if_file_revision`
- [x] 292. `ends_with_if_file_revision`
- [x] 293. `has_nul_if_file_revision`
- [x] 294. `xor_parity_if_file_revision`
- [x] 295. `fill_range_if_file_revision`
- [x] 296. `xor_range_if_file_revision`
- [x] 297. `reverse_range_if_file_revision`
- [x] 298. `rotate_left_if_file_revision`
- [x] 299. `rotate_right_if_file_revision`
- [x] 300. `replace_first_literal_if_file_revision`

### Boundaries and required verification

- Service planning computes dependency order on a temporary copy. It does **not** forge a successful PID1 or scheduler start.
- IPC methods continue to require identities supplied by trusted kernel ingress; sender PID alone is not sender-generation identity.
- Keyboard queue protections are not evidence of physical PS/2 interrupt delivery or a mature accessibility/input stack.
- Window stacking is fixed-array registration order; there is no complete isolated ring-3 compositor.
- RAM filesystem edits are real in-memory bytes only, **not durable ATA-backed writes or recovery**. XOR parity is noncryptographic.
- Retest all existing and new Rust unit tests, no_std builds, adversarial security, ELF/ISO boot and runtime QEMU checks at the amended head SHA. Never merge unless all required CI gates pass.


## R085 extension IV — 100 **new planned acceptance tasks** (#301–400)

**Status:** All 100 entries below are **PLANNED / NOT IMPLEMENTED**. This addition is a substantive M1 implementation checklist, **not** 100 new Rust methods or 100 completed functionality claims. It decomposes existing M1 gates (#007–100) into independently testable requirements. It should not be double counted as 100 extra product features. Use the same PR #732, five logical ownership workstreams and one consolidated source commit; no five independent agents were launched. Do not merge until exact-head GitHub checks pass. This extension modifies documentation only.

**Accounting (mixed granularity):** entries #001–300 already existed (226 source-level checked / 74 uncompleted higher-level acceptance tasks); #301–400 add 100 unchecked acceptance subtasks. Total: **400 checklist entries, 226 source-level checked and 174 unchecked**. Actual M1 gate completion remains evidence-based, independently of these counts.

**Priority:** Complete unfinished M1 integration gates before unrelated M2/compatibility/UI feature batches. Each task below requires real production source, targeted regression and integrated QEMU/guest evidence where specified; acceptance is not earned by docs/tests alone.

### A. Normal boot, PID1 process supervision and scheduler integration (P0)

**Related gate:** Close 007–020 with actual ordinary-boot behaviour, not policy-only Rust unit tests.

- [ ] 301. **Versioned BootInfo ingress** — reject unexpected BootInfo ABI versions before PID1 service startup; test valid and malformed firmware handoffs.
- [ ] 302. **Boot memory handoff ownership** — reserve firmware, framebuffer and initrd regions before releasing the bootloader memory map; run QEMU overlap-negative tests.
- [ ] 303. **Kernel-to-user ELF entry validation** — reject a noncanonical, writable-executable or kernel-address program entry before scheduling PID1.
- [ ] 304. **PID1 identity allocation** — assign a unique process generation and service handle in the actual scheduler, rejecting duplicate live identities.
- [ ] 305. **Service manifest dependency import** — load a signed/built-in manifest into the validated graph and fail normal boot on missing mandatory edges.
- [ ] 306. **Startup CPU and memory budget admission** — refuse launching a service whose declared quotas exceed currently available protected resources.
- [ ] 307. **Actual child creation handoff** — connect validated service admission to real ring-3 ELF load, PCB insertion and scheduler handoff.
- [ ] 308. **Authenticated readiness message** — accept READY only from the launched child’s current PID, generation and expected service handle.
- [ ] 309. **Startup timeout clock source** — use the real monotonic timer to close a launch attempt rather than a host-only simulation counter.
- [ ] 310. **Startup failure taxonomy** — distinguish image rejection, allocation failure, crash, deadline expiration and dependency failure in machine-readable status.
- [ ] 311. **Bounded restart backoff** — enforce deterministic exponential backoff and a fixed restart ceiling for flapping optional services.
- [ ] 312. **Essential failure dependency shutdown** — stop or quarantine dependent children in reverse dependency order when an essential parent dies.
- [ ] 313. **Recovery mode service allowlist** — verify that failure boots only minimal signed recovery services, not the full optional desktop stack.
- [ ] 314. **Boot retry idempotence** — make repeated supervisor start requests reuse or reject the existing process rather than double-spawning.
- [ ] 315. **Child exit and zombie collection** — reclaim a failed child’s memory, handles, scheduler slot and service ownership exactly once.
- [ ] 316. **Readiness-gated capability handoff** — transfer service capabilities only after the kernel authenticates its READY transition.
- [ ] 317. **Crash reason durability boundary** — save a bounded recovery reason to the approved device-backed crash store without overwriting protected partitions.
- [ ] 318. **Serial lifecycle trace correlation** — include boot ID, PID generation, service ID and state transition in real guest UART logs.
- [ ] 319. **Repeated normal boot acceptance** — cold-boot the normal profile ten times in QEMU with distinct service ordering assertions and artifacts.
- [ ] 320. **Crash-to-recovery acceptance** — inject a PID1/essential-service failure and verify real QEMU recovery-shell entry, no restart loop and safe cleanup.

### B. Isolated ring-3 compositor, protected surfaces and physical input (P0)

**Related gate:** Close 048–060 and 066–080 with authentic ring-3 rendering and PS/2 evidence; preserve existing kernel-owned graphics until proven replaced.

- [ ] 321. **Compositor ELF runtime** — execute the compositor as its own ring-3 process with a distinct address space and supervisor-owned lifecycle.
- [ ] 322. **Opaque surface capability handles** — create per-client handle IDs bound to PID generation; reject cross-process guessed handles.
- [ ] 323. **Page-permission surface isolation** — map client frame pages with minimal read/write rights and verify foreign ring-3 reads fault.
- [ ] 324. **Guard-page surface boundary** — add unmapped guard pages around mapped frame buffers and test overrun handling in QEMU.
- [ ] 325. **Revocable shared-memory mapping** — unmap and invalidate a client’s frame pages when its window or task exits.
- [ ] 326. **Versioned surface-create protocol** — define and test a bounded request/reply message with width, height, stride and format validation.
- [ ] 327. **Damage queue capacity contract** — use fixed-size per-surface damage queues with explicit overflow/coalescing behaviour.
- [ ] 328. **Invalid rectangle normalization** — reject negative-equivalent or wrapped coordinates and prevent overflow during rectangle clipping.
- [ ] 329. **Frame-present acknowledgment** — signal a client only after consuming a valid frame revision; reject stale or duplicate presentations.
- [ ] 330. **Surface-handle spoofing regression** — run two hostile ring-3 clients and prove one cannot present or destroy the other’s surface.
- [ ] 331. **Z-order atomic publication** — synchronize authenticated window stacking with compositor presentation without torn focus/z-order state.
- [ ] 332. **Cursor coordinate transform** — validate actual PS/2 mouse coordinates against the current framebuffer scale and viewport.
- [ ] 333. **Pointer capture lease** — release mouse drag/capture on client crash, button-up, focus change and compositor restart.
- [ ] 334. **Bounded mouse motion coalescing** — compress high-rate PS/2 movement while preserving button transitions in full queues.
- [ ] 335. **Keyboard focus seat ownership** — route the real keyboard source to only the currently authenticated surface owner.
- [ ] 336. **Modifier and repeat reset** — clear keyboard modifier/repeat state after device reset or compositor crash to prevent stuck keys.
- [ ] 337. **Input flood fairness** — keep terminal and Files input responsive during sustained key/mouse IRQ stress in QEMU.
- [ ] 338. **Compositor fallback frame** — show a bounded recovery frame when the user-space compositor dies without freezing the kernel shell.
- [ ] 339. **Real compositor screenshot proof** — capture a QEMU frame with compositor and at least two independent client PIDs visible in serial logs.
- [ ] 340. **Restart-after-draw proof** — restart the compositor and show screenshot/serial evidence of surface revocation, redrawing and correct restored focus.

### C. Durable hierarchical VFS, terminal persistence and recovery (P0)

**Related gate:** Refine 084–100 with device-backed, reboot-proven semantics; RAMFS-only methods do not count.

- [ ] 341. **Mounted volume identity contract** — mount an expected disk UUID and filesystem revision, rejecting substituted QEMU volumes.
- [ ] 342. **Partition table input bounds** — reject out-of-range LBA and overlapping protected partition extents before issuing device I/O.
- [ ] 343. **Canonical path at syscall boundary** — normalize actual copied user pointers and reject traversal before any VFS lookup.
- [ ] 344. **Unsupported link fail-closed** — reject symlinks/hardlinks unless their resolution and permission semantics are explicitly implemented.
- [ ] 345. **Generation-bound open-file handles** — bind every VFS handle to exact process generation and a per-handle access mask.
- [ ] 346. **Exec/launch handle inheritance rule** — explicitly define, test and default-deny file-descriptor inheritance into newly launched ELFs.
- [ ] 347. **Exit-time file-handle revocation** — close a dead process’s file handles, pending writes and directory cursors exactly once.
- [ ] 348. **Global write-guard insertion** — enforce protected namespaces at mkdir, create, rename, truncate, append and delete entry points.
- [ ] 349. **ACL inheritance at create** — derive new inode ownership/rights from authenticated user and parent policy, not user-provided claims.
- [ ] 350. **Per-user home path enforcement** — restrict a nonprivileged process to the approved user namespace and prove denial on sibling paths.
- [ ] 351. **Temporary-file atomic replace** — write to a private temp inode, flush and rename atomically without exposing partially replaced files.
- [ ] 352. **Journal commit checksum** — attach generation, length and integrity metadata to actual block transaction records.
- [ ] 353. **Mirrored metadata generation selection** — choose the newest fully valid journal mirror and reject mismatched/corrupt generations.
- [ ] 354. **Mid-sector write fault injection** — simulate a torn sector write and verify old-or-new consistency after a cold reboot.
- [ ] 355. **Disk-full rollback** — force volume exhaustion during create/rename and prove the old file and directory structure remain readable.
- [ ] 356. **Verified flush ordering** — trace block flush barriers before and after journal publication, including a missing-flush negative case.
- [ ] 357. **Read-only degraded mount** — quarantine corrupt journal metadata and expose filesystem state without silently writing repairs.
- [ ] 358. **Directory enumeration ownership** — prevent unprivileged listing of unauthorized directory contents and stale generation cursors.
- [ ] 359. **Real terminal durable CRUD script** — execute create/list/read/rename/delete in userland against disk-backed VFS, reboot and compare snapshots.
- [ ] 360. **Reboot and power-cut matrix** — automate at least ten reboot/fault cut points with checksummed pre/post file and metadata evidence.

### D. Syscall ABI, authenticated IPC and privilege separation (P0)

**Related gate:** Integrate unfinished 026–040 with scheduler identity, bounded user copy and real hostile ring-3 workloads.

- [ ] 361. **Syscall ABI version negotiation** — reject unknown syscall protocol versions without interpreting untrusted layout as current structures.
- [ ] 362. **Syscall structure length validation** — enforce exact minimum/maximum struct lengths before copying request data from ring-3.
- [ ] 363. **Full user pointer span validation** — validate every page in a nonzero pointer/length range for overflow and user mapping.
- [ ] 364. **Copy-out zero-initialization** — ensure kernel padding/pointer fields cannot leak to an untrusted caller on success or failure.
- [ ] 365. **Handle type tagging** — distinguish IPC, file, surface and process handles so rights from one type never authorize another.
- [ ] 366. **Per-call capability checks** — validate actual scheduler-owned handle rights before attempting filesystem, IPC or window actions.
- [ ] 367. **Sender generation from PCB** — derive both sender PID and lifetime generation from the current scheduler PCB, never caller bytes.
- [ ] 368. **Receiver generation routing** — look up target PID generation under a stable task reference before enqueue and wakeup.
- [ ] 369. **Blocking receive cancellation** — wake an IPC waiter on target exit, capability revocation or explicit timeout with a typed error.
- [ ] 370. **Bounded mailbox backpressure** — distinguish queue-full from receiver-dead errors while retaining unrelated queued messages.
- [ ] 371. **Fair IPC wakeup** — test no permanent starvation between two runnable message consumers under timer preemption.
- [ ] 372. **At-most-once acknowledgment token** — reject replayed request identifiers after successful delivery without dropping unrelated messages.
- [ ] 373. **Per-process IPC quota enforcement** — bound outstanding messages and bytes by actual process identity and rights.
- [ ] 374. **Cross-process send denial** — prove nonauthorized ring-3 clients cannot inject events into the compositor or PID1 mailbox.
- [ ] 375. **Explicit capability transfer** — require the sender’s transfer right and target-specific receiver confirmation for handle duplication.
- [ ] 376. **Revocation propagation** — invalidate transferred child handles and blocked work when the parent authority is revoked.
- [ ] 377. **Task-exit race safety** — cover simultaneous IPC send, process teardown and PID reuse without use-after-free or stale delivery.
- [ ] 378. **Lock-order instrumentation** — assert scheduler/mailbox/VFS lock hierarchy in debug regressions and avoid blocking under IRQ-disabled locks.
- [ ] 379. **Deterministic adversarial syscall corpus** — replay malformed lengths, stale handles, pointer overflow and wrong rights with exact expected errno.
- [ ] 380. **Real two-client QEMU isolation proof** — execute conflicting ring-3 file/IPC/surface calls and capture serial proof of no cross-PID access.

### E. Real hardware-driver boundaries, QEMU CI and recovery evidence (P0)

**Related gate:** Convert individual Rust policy tests into exact-SHA boot, driver, fault and diagnostic evidence; do not duplicate previously verified basic demos.

- [ ] 381. **Firmware address-range inspection** — log validated UEFI/Limine framebuffer, MMIO, RAM and reserved regions before native driver probing.
- [ ] 382. **Interrupt-controller selection guard** — make PIC/APIC mode decisions explicit and reject contradictory IRQ ownership at boot.
- [ ] 383. **IRQ acknowledgment auditing** — assert each handled hardware IRQ is acknowledged exactly once, including error paths.
- [ ] 384. **Timer watchdog boundedness** — prove timed-out device operations return control without deadlocking the scheduler.
- [ ] 385. **PS/2 IRQ queue pressure test** — deliver burst input at hardware-emulated speed and verify safe bounded overflow handling.
- [ ] 386. **ATA IDENTIFY timeout fault** — inject no response and ensure the kernel offers read-only recovery or a clear device error.
- [ ] 387. **ATA flush confirmation test** — check real IDE flush completion and surface hard failure before acknowledging durable file updates.
- [ ] 388. **Torn mirror fault harness** — corrupt each metadata mirror separately and prove valid-generation recovery on actual guest storage.
- [ ] 389. **Protected raw-block policy** — deny ordinary user-mode operations targeting boot/recovery blocks even when issuing direct device calls.
- [ ] 390. **Mount failure recovery pathway** — boot to a minimal console when the expected data device is absent, read-only or corrupt.
- [ ] 391. **Disposable QEMU disk artifact isolation** — create per-test snapshots so parallel CI jobs cannot share or mutate the same guest image.
- [ ] 392. **Cold boot reproducibility** — reboot with cleared emulated memory and verify persistent data was read from disk, not retained RAM.
- [ ] 393. **Tagged guest serial event format** — emit machine-parseable boot ID, exact SHA, profile, fault category and assertion event IDs.
- [ ] 394. **Kernel panic evidence preservation** — bound captured panic context and verify it is displayed without exposing another process’s secrets.
- [ ] 395. **Device disconnect injection** — remove/unavailable the virtual disk mid-test and prove bounded errors rather than kernel panic loops.
- [ ] 396. **Unknown PCI device fail-closed** — enumerate an unsupported virtual PCI function and prove no unsafe MMIO or IRQ binding.
- [ ] 397. **No-GUI headless boot acceptance** — verify server/recovery startup does not automatically activate the compositor or PS/2 GUI service.
- [ ] 398. **ELF/ISO deterministic build budget** — record kernel binary sizes and compare defined thresholds without disabling compilation tests.
- [ ] 399. **Artifacts for real frame and process proof** — archive QEMU frame captures with matching serial logs and commit SHA, not mockup images.
- [ ] 400. **Single-SHA release gate matrix** — require host tests, no_std build, normal PID1 boot, compositor, durable reboot and QEMU failure recovery to pass on the exact integration SHA before merge.

### Completion rules for #301–400

1. Start work on the eight remaining high-level M1 gates from the canonical `docs/IMPLEMENTATION_BACKLOG.md` and issue #616; these subtasks refine those gates rather than redefining their completion.
2. Each completed subtask must be associated with the exact Rust production changes, test names and CI workflow/run references, with actual output from ordinary boot and real ring-3 execution where demanded.
3. Require original protection rules, Rust/no_std compilation, security regressions, QEMU boot and power-cut evidence at the final PR SHA. Never disable checks, mark an unchecked item completed from policy-only helper code, or claim simulated UI screenshots are QEMU evidence.
4. Maintain exactly one source commit in PR #732 if continuing the existing batch, and leave the PR draft while mandatory checks are running or failing. If the integration branch has moved, inspect and reconcile before updating it.


## R085 extension V — next 100 **planned M1 acceptance subtasks** (#401–500)

**Status: ALL #401–500 TASKS ARE PLANNED (unchecked).** This is a checklist-only expansion; no 100-feature implementation is claimed. The 100 entries strengthen M1 process lifetime, real Terminal/Files workflows, persistent VFS semantics, memory/ABI security and reproducible QEMU acceptance. They refine the canonical remaining M1 gates and do not replace those gate definitions in `docs/IMPLEMENTATION_BACKLOG.md` or issue #616. Five workstreams describe ownership domains, **not five simultaneously running AI agents**.

**Accounting:** previously #001–400 contained 226 source-implemented (`[x]`, not necessarily CI-verified) and 174 unchecked items. This extension adds 100 unchecked items for **500 unique checklist entries: 226 source-implemented / 274 unchecked**. Mixed-granularity task counts are not a percentage of OS completion. No new Rust production code in this documentation-only extension.

### A. Process lifetime, scheduler correctness and controlled boot (P0) (#401–420)

- [ ] 401. **PID1 stack guard acceptance** — map an unmapped lower stack guard and prove a deliberate ring-3 stack overflow kills only the service, not the kernel.
- [ ] 402. **Service argv/env copy isolation** — copy launch parameters into owned user memory; mutate the parent buffer afterward and prove the child sees a stable snapshot.
- [ ] 403. **ELF program-header overlap rejection** — reject overlapping PT_LOAD ranges before address-space insertion; preserve prior process maps on failure.
- [ ] 404. **W^X across ELF segments** — reject executable-writable segment requests and verify final user page permissions in QEMU page-table diagnostics.
- [ ] 405. **Per-process handle-table teardown** — close IPC, surface and VFS handles exactly once in one task-exit path; detect duplicate releases with counters.
- [ ] 406. **Scheduler run-queue invariant checks** — verify each runnable thread appears at most once under yield, preemption, blocking and wakeup transitions.
- [ ] 407. **Sleep/wakeup monotonic deadline** — cover early timer ticks, wrap-safe deadline arithmetic and exact-once wakeup of a sleeping user thread.
- [ ] 408. **Blocked child reparenting** — when a non-PID1 parent dies, explicitly adopt or reap its sleeping children under the selected supervisor policy.
- [ ] 409. **PID generation wrap strategy** — define and test behavior at generation-counter exhaustion so stale handles can never become valid again.
- [ ] 410. **Process identifier allocation exhaustion** — simulate all PID slots occupied, fail creation without partial allocations and recover after a task exits.
- [ ] 411. **Kernel thread versus user thread identity** — enforce typed separation so user syscalls cannot target scheduler-only maintenance threads.
- [ ] 412. **Supervisor status snapshot consistency** — return a stable multi-service state snapshot even while children transition between READY and EXITED.
- [ ] 413. **Stopped-service wait semantics** — ensure wait returns the correct exit reason once and differentiates never-started, live and already-reaped IDs.
- [ ] 414. **Restarted service state reset** — rebuild private handles, failure counters and readiness state using the new process generation after an approved restart.
- [ ] 415. **Optional-service dependency suppression** — avoid starting optional children when a required parent is disabled; report a nonfatal dependency status.
- [ ] 416. **Service startup concurrency limit** — bound simultaneously loading child images to prevent the supervisor from exhausting PMM or kernel stacks.
- [ ] 417. **Preemption across ELF loading** — stress timer interrupts while allocating/mapping a child to prove no half-built PCB becomes runnable.
- [ ] 418. **Scheduler idle-path proof** — show the CPU enters an idle state when no threads are runnable and reliably resumes on a guest IRQ.
- [ ] 419. **Boot-profile state audit** — verify recovery/normal/headless profile selection is carried into PID1 without mutable user-controlled policy flags.
- [ ] 420. **Boot service ownership trace** — generate a per-service launch-to-exit trace in actual QEMU guests and reconcile every resource allocation with one cleanup event.

### B. Terminal, Files and actual compositor integration (P0) (#421–440)

- [ ] 421. **Terminal-to-VFS open dialogue** — connect graphical Terminal's real open command to a generation-bound VFS handle and display exact errno on failure.
- [ ] 422. **Terminal directory traversal** — make pwd/cd/list resolve the actual mounted hierarchy and reject inaccessible path changes without altering cwd.
- [ ] 423. **Terminal large-output pagination** — stream directory and file output through a bounded scrollback without blocking the compositor event queue.
- [ ] 424. **Terminal UTF-8 input boundary** — reject incomplete or malformed UTF-8 command sequences while preserving exact user-visible edit position.
- [ ] 425. **Terminal paste quota** — paste bounded multiline text without exceeding terminal, IPC or input buffers, with a visible truncation/error state.
- [ ] 426. **Terminal process-status command** — report authenticated supervisor-owned process identity, lifetime and state rather than fabricated UI values.
- [ ] 427. **Files tree live refresh** — invalidate stale directory results after a successful rename, delete or external process modification.
- [ ] 428. **Files permission-denied feedback** — expose access-denied metadata in an in-app error surface without displaying protected path contents.
- [ ] 429. **Files selection after rename** — preserve or clear selection by stable object identity following authenticated rename; never rebind to a reused path.
- [ ] 430. **Files copy progress and cancellation** — stream bounded copy progress with cancellation and cleanup of incomplete destination data.
- [ ] 431. **Files move across volumes** — reject unsupported cross-volume atomic rename or implement explicit safe copy-verify-delete with recoverable error reporting.
- [ ] 432. **Files conflict-resolution workflow** — require explicit overwrite/rename/skip on existing target and recheck target version before commit.
- [ ] 433. **Files disk-full error UI** — show actual quota/device exhaustion and leave the source selection and prior files unchanged.
- [ ] 434. **Text-editor unsaved-state guard** — block accidental close of dirty editor buffers and offer save/discard/cancel through real window IPC.
- [ ] 435. **Text-editor revision conflict** — detect file changes made by another client and refuse silent overwrite of stale disk content.
- [ ] 436. **Cross-window keyboard shortcuts** — scope Ctrl+C/Ctrl+V and application shortcuts to the focused authenticated window without copying foreign clipboard contents.
- [ ] 437. **Compositor resize redraw contract** — request a fresh client frame when a surface size changes instead of stretching untrusted stale dimensions.
- [ ] 438. **Terminal and Files crash isolation** — crash either client ELF and verify the surviving client remains interactive and its surface intact.
- [ ] 439. **Compositor reconnect handshake** — require surviving clients to re-register capabilities after compositor restart before any draw/input delivery.
- [ ] 440. **Usable default desktop smoke test** — record a real QEMU user journey opening Terminal, creating a file, viewing it in Files and editing it, with frame/serial evidence.

### C. Persistent storage and VFS semantics with hostile user workloads (P0) (#441–460)

- [ ] 441. **VFS directory-handle cursor lease** — bind enumerators to inode identity and revision; reject reused/deleted cursors after unmount or rename.
- [ ] 442. **Path separator canonical-equivalence corpus** — enforce one canonical interpretation of repeated slashes, dot segments, NULs and invalid bytes.
- [ ] 443. **No-escape mountpoint traversal** — prove relative traversal from a mounted subtree cannot escape into the parent filesystem namespace.
- [ ] 444. **Mount namespace detach semantics** — reject or defer unmount with live handles and cleanly invalidate them when a forced recovery detach is allowed.
- [ ] 445. **Create-with-exclusive semantics** — implement O_EXCL-like atomic create so two concurrent client attempts never both believe they created the same file.
- [ ] 446. **Open-with-truncate guard** — require authenticated write rights before mutation and avoid destroying existing bytes on failed allocation.
- [ ] 447. **Append position integrity** — serialize concurrent appenders so a complete record is written once without offset overlap.
- [ ] 448. **Sparse-write policy** — explicitly reject oversized holes or implement a bounded, zero-filled gap with quota accounted before publication.
- [ ] 449. **Partial block read semantics** — return only initialized bytes and a precise count when a request spans the end of a file.
- [ ] 450. **Short-write visibility** — distinguish user-copy failure, quota error and device short-write without reporting durable success prematurely.
- [ ] 451. **Durable file-close semantics** — define whether close flushes pending data; propagate a failed flush back to the caller rather than silently hiding it.
- [ ] 452. **Rename replace policy** — define and test replacement of existing files, same-inode no-op, type mismatch and cross-directory permission checks.
- [ ] 453. **Atomic directory creation path** — validate all missing intermediate components and do not expose a half-created hierarchy on capacity failure.
- [ ] 454. **Directory deletion nonempty guard** — reject rmdir when children remain and preserve metadata if a concurrent create races the deletion.
- [ ] 455. **Filesystem link-count consistency** — if links are unsupported reject them; otherwise retain correct inode lifetime across unlink and open handles.
- [ ] 456. **Versioned inode read-after-rename** — keep an already-open file handle bound to its inode while resolving its path by the new directory entry.
- [ ] 457. **Checksum mismatch isolation** — on one corrupt data block, return a typed error for that file without damaging unrelated clean extents.
- [ ] 458. **Recovery journal idempotent replay** — replay the same valid committed record twice and prove neither duplicate directory entries nor extra allocation.
- [ ] 459. **Free-space accounting after crash** — reconcile reclaimed blocks and inode counters after interrupted delete/replace without leaking capacity.
- [ ] 460. **Durable multi-client QEMU test** — have Terminal and Files concurrently create, rename and edit disjoint files; reboot and compare byte-accurate outputs.

### D. Kernel memory safety, authorization and user ABI lifecycle (P0) (#461–480)

- [ ] 461. **Zeroed frame on new user mapping** — prove newly allocated user frames do not expose bytes from a previously terminated process.
- [ ] 462. **User memory unmap ownership** — refuse removal of a kernel/shared/foreign mapping by a ring-3 task and preserve its valid local mappings.
- [ ] 463. **Copy-on-write fault isolation** — where COW is enabled, force a write fault and verify private page duplication without mutating sibling mappings.
- [ ] 464. **Executable page transition check** — require a trusted loader transition before any writable user page becomes executable.
- [ ] 465. **User pointer aliasing regression** — test input/output buffers that fully and partially overlap so copies cannot corrupt kernel-owned state.
- [ ] 466. **Unaligned syscall request acceptance** — either safely copy unaligned structures or reject them consistently with a documented errno.
- [ ] 467. **Oversized vector I/O accounting** — bound sum of iovec lengths with checked arithmetic and reject overlapping output segments where unsupported.
- [ ] 468. **Nested handle lifetime audit** — release handles embedded in IPC attachments and shared-surface records when their parent container is revoked.
- [ ] 469. **Owner-scoped handle enumeration** — return only caller-visible handles, using a bounded copied snapshot and never exposing kernel pointers.
- [ ] 470. **Syscall interruption contract** — define retryable versus terminal errno when a user thread is killed or signaled during a blocking operation.
- [ ] 471. **Per-process capability budget** — deny excess rights/handle allocations before mutation and restore the count on failed publication.
- [ ] 472. **Capability duplicate attenuation** — allow duplication only with equal-or-fewer rights and reject any attempted privilege amplification.
- [ ] 473. **Revocation versus in-flight operation** — define the linearization point when a capability is revoked during a filesystem or IPC call.
- [ ] 474. **User page fault diagnostic isolation** — report failing PID and virtual address safely in serial logs without disclosing kernel stack bytes.
- [ ] 475. **Demand allocation OOM rollback** — fail a mapping request without leaving half-installed PTEs, leaked frames or a runnable corrupted task.
- [ ] 476. **Read-only executable image sharing** — if image pages are shared, prove client writes cannot alter another process's code/data.
- [ ] 477. **Kernel stack exhaustion containment** — enforce minimum interrupt/syscall stack headroom and produce a controlled diagnostic for overflow.
- [ ] 478. **Interrupt-return privilege check** — verify user-controlled saved registers cannot return from syscalls into supervisor privilege or forbidden segments.
- [ ] 479. **Per-syscall audit rate limit** — bound adversarial error-log bursts without suppressing a unique critical privilege violation.
- [ ] 480. **Combined ring-3 hostile-client suite** — run incorrect rights, stale PID generations, invalid mappings and malicious IPC payloads simultaneously with no kernel panic.

### E. QEMU evidence quality, hardware recovery and integration CI (P0) (#481–500)

- [ ] 481. **CI toolchain provenance** — record rustc/cargo, linker, firmware and QEMU versions used for the exact source SHA.
- [ ] 482. **Boot-image contents manifest** — emit deterministic hash and inventory of the bootloader, kernel ELF, initrd and ISO inputs in CI artifacts.
- [ ] 483. **Guest serial output bounds** — cap logs by bytes and time while retaining last-failure context; fail cleanly on a silent hung guest.
- [ ] 484. **Guest assertion sequence checking** — reject a fake 'pass' marker printed before the expected PID1, syscall and cleanup events occur.
- [ ] 485. **QEMU clock timeout reproducibility** — test watchdog behavior under deliberately slow host runners without masking a true guest deadlock.
- [ ] 486. **Multi-profile matrix isolation** — boot normal, recovery and headless profiles from the same SHA with separate disk and firmware state.
- [ ] 487. **Framebuffer screenshot integrity** — attach pixel dimensions and an image checksum to each screenshot and correlate it with guest serial boot ID.
- [ ] 488. **Native artifact size regression** — report changes in code, data, bss and ISO size with a declared budget and machine-readable trend.
- [ ] 489. **Host-vs-guest evidence separation** — mark host Rust unit tests separately from QEMU assertions so host passes never imply guest boot.
- [ ] 490. **No_std target feature audit** — fail CI on accidental std dependencies or unsupported target CPU features in freestanding modules.
- [ ] 491. **Fault-injection seed replay** — capture and rerun the same deterministic storage/IRQ/syscall fuzz seed on a failed integration SHA.
- [ ] 492. **QEMU disk-write trace matching** — prove the guest issued real block writes and flushes before a durability test asserts persistence.
- [ ] 493. **Reboot readback provenance** — match checksum, file revision and disk image ID from before shutdown to a fresh emulator process after reboot.
- [ ] 494. **Hardware resource conflict fixture** — emulate competing PCI MMIO/IRQ claims and verify the driver manager rejects the second claimant.
- [ ] 495. **ACPI table rejection suite** — supply corrupt lengths/checksums and prove the kernel does not parse untrusted firmware pointers.
- [ ] 496. **Memory-map fragmentation stress boot** — launch QEMU with constrained and fragmented memory and prove bounded PMM allocation failure.
- [ ] 497. **Unclean shutdown regression collection** — archive raw disk snapshots and repair decisions at several interruption boundaries for postmortem reproduction.
- [ ] 498. **Crash recovery user-visible acceptance** — after injected failure, show a real guest recovery prompt with diagnosis and safe read-only file access.
- [ ] 499. **Mandatory check identity audit** — require workflow status checks correspond to the current integration head SHA rather than stale green parent runs.
- [ ] 500. **M1 gate-evidence index** — link each original unfinished M1 high-level gate to actual test case IDs, QEMU run URLs and retained artifacts without inventing completion.

### Exit criteria for the extension

1. All new entries start as **unchecked**; mark one checked only after real source integration, targeted tests and exact-SHA validation evidence are attached.
2. Prioritize the already open M1 acceptance gates over increasing count of small Rust helper APIs. Integrate with real boot, ring-3 components and durable device-backed storage.
3. Preserve the existing single-commit PR #732 policy: one consolidated commit and no additional PR. Do not merge on pending/failed required checks, or bypass failing security/boot tests.
4. Five logical workstreams do not establish five independently spawned agents. No claim that an OS gate is completed without the actual QEMU, disk reboot or hardware evidence demanded by that gate.


## R085 extension VI — 100 additional **planned acceptance tasks** (#501–600)

**Status: PLANNED, not implemented.** These 100 entries are new work items, not 100 additional Rust methods, green builds, fully integrated OS features, or closed M1 gates. They refine the original acceptance-first kernel roadmap; the authoritative M1 gate tracker and `docs/IMPLEMENTATION_BACKLOG.md` still determine what is actually completed.

**Sequencing:** prioritize ordinary PID1 boot, ring-3 compositor/input and power-loss-safe VFS first. Broader K06 socket hosting and secure-update foundations are designed now but should not divert implementation capacity while the urgent M1 blockers remain open.

**Before this update:** 500 unique checklist items, 226 source-implemented/checkmarked and 274 open. **After this checklist-only update:** 600 unique items, 226 source-implemented/checkmarked and 374 open. Neither count can be turned into an IndianOS percentage. Five workstreams are logical ownership only; no five independent agents were launched.


### A. Kernel virtual memory, CPU isolation and scheduler integration (P0)

Close actual PMM/VMM and thread-lifetime gaps. Each task requires freestanding/no_std checks and a guest proof when CPU, page table or scheduler behavior is claimed.

- [ ] 501. **Frame reference ownership ledger** — associate every shared or private PMM frame with an owner/reference count and prove last-reference release exactly once after user-task exit.
- [ ] 502. **Boot memory map overlap arbitration** — reject overlapping regions with conflicting types before PMM initialization instead of choosing one silently; run malformed QEMU boot-map fixtures.
- [ ] 503. **Usable-memory coalescing guard** — coalesce only adjacent compatible memory map entries without including ACPI, DMA/MMIO or reserved holes; assert preserved byte accounting.
- [ ] 504. **Per-address-space page-table lifecycle** — allocate, switch and destroy independent PML4 roots with no kernel mapping teardown or residual user mapping after process exit.
- [ ] 505. **TLB invalidation on permission downgrade** — revoke a user page’s write/execute rights and prove the old cached translation cannot still perform the operation.
- [ ] 506. **Remap-without-leak transaction** — stage page-table edits, reject conflicts and restore original mappings and frame counters on any mid-operation failure.
- [ ] 507. **Guarded user stack expansion** — grow a stack only within an explicit maximum and abort runaway recursion at the guard boundary without corrupting adjacent VMAs.
- [ ] 508. **Lazy anonymous page commitment** — reserve virtual address space without eagerly consuming RAM and charge physical pages only on validated demand faults.
- [ ] 509. **Out-of-memory victim policy** — specify deterministic task failure versus recovery-mode entry for nonessential and essential allocations; capture memory counters in QEMU.
- [ ] 510. **Kernel heap integrity canaries** — detect overwritten allocation sentinels in debug builds and report an isolated corruption diagnosis before reuse.
- [ ] 511. **DMA-capable frame quarantine** — mark pinned DMA frames unavailable for user allocation until the driver has completed and unmapped all device transfers.
- [ ] 512. **Memory pressure supervisor notification** — publish bounded free-frame thresholds to PID1 to stop optional launches before essential kernel allocations fail.
- [ ] 513. **Per-task address-space mapping quota** — reject user mapping storms without exhausting kernel page tables or exposing another task’s address range.
- [ ] 514. **Copy-from-user fault rollback** — handle a fault in the middle of multi-page input copying without mutating destination kernel state or keeping pages pinned.
- [ ] 515. **Copy-to-user partial-output policy** — preflight a multi-page result and guarantee the documented no-partial-output rule on invalid destination pages.
- [ ] 516. **Kernel stack high-water accounting** — record maximum interrupt/syscall stack use under nested interrupts and fail an explicit budget gate before exhaustion.
- [ ] 517. **Scheduler priority-inversion test harness** — exercise a low-priority lock owner interrupted by a high-priority waiter and prove bounded priority inheritance or an explicit lock-policy fallback.
- [ ] 518. **Tickless idle timer programming** — use the earliest real sleeping-task deadline when the run queue is empty, and retain a safe PIT/APIC fallback if unsupported.
- [ ] 519. **CPU exception nesting policy** — give double-fault, page-fault and general-protection paths distinct guarded stacks so a nested exception cannot overwrite the original trap frame.
- [ ] 520. **Real PMM/VMM stress acceptance** — run concurrent ring-3 allocations, remaps, faults and task exits in QEMU and require unchanged free-frame baselines after cleanup.

### B. Real graphical desktop, app sessions and compositor reliability (P0)

Prioritize actual ring-3 compositor ownership, Terminal/Files communication and framebuffer evidence, not UI mockups or isolated policy helper methods.

- [ ] 521. **Framebuffer format negotiation** — read hardware-provided pixel format, stride and dimensions before composing and refuse unsupported mode combinations with a serial diagnostic.
- [ ] 522. **Dirty-tile occlusion calculation** — avoid repainting entirely obscured surface regions while preserving correct damage invalidation when top windows move.
- [ ] 523. **Front/back buffer handoff** — publish a complete compositor frame from a private back buffer so user-visible pixels cannot mix old and new client revisions.
- [ ] 524. **Frame presentation timeout recovery** — detect a client that never acknowledges surface requests and continue presenting other healthy clients without stalling the desktop.
- [ ] 525. **Per-window redraw budget** — bound how much CPU time one busy renderer consumes in each compositor turn so Terminal remains responsive.
- [ ] 526. **Client surface format rejection** — validate RGBA/pixel layout, stride overflow and byte alignment at the real IPC/VM mapping boundary, not merely geometry structs.
- [ ] 527. **Visible-region clipping** — clip all client draw batches to the window content rectangle after accounting for title bars, borders and display edges.
- [ ] 528. **Per-client render command throttle** — rate-limit hostile render submissions without accidentally starving input or displaying unvalidated memory.
- [ ] 529. **Modal-dialog focus ownership** — route keyboard and pointer focus to an authenticated modal child while preventing unrelated applications from hijacking the active dialog.
- [ ] 530. **Transient parent/child window cleanup** — close dialog/tooltip surfaces atomically when their parent process terminates, including focus-stack restoration.
- [ ] 531. **Desktop clipboard capability broker** — introduce size-limited copy/paste with explicit source/target permissions; reject clipboard reads from unfocused or unauthorized clients.
- [ ] 532. **Clipboard data revocation on logout** — zero and retire session clipboard bytes before a new user session is allowed to read any clipboard handle.
- [ ] 533. **Compositor theme resource validation** — verify bundled icon/fonts/colors have bounded lengths and safe fallbacks so a malformed theme cannot crash normal boot.
- [ ] 534. **Terminal resize reflow correctness** — preserve visible cursor position and scrollback lines across repeated QEMU window resizing without losing typed command text.
- [ ] 535. **Files drag-and-drop permission check** — authorize any file move at the receiving directory and recheck inode revisions before committing the operation.
- [ ] 536. **Editor crash-resilient draft recovery** — write bounded editor drafts to an authenticated recovery location and restore only matching user/session revisions.
- [ ] 537. **Window minimize resource policy** — retain required client state while limiting hidden-surface frame allocations; restore with a fresh frame before accepting input.
- [ ] 538. **Multi-client pointer capture arbitration** — ensure a drag initiated in one client cannot send synthetic move events into another client’s window.
- [ ] 539. **Real DPI-independent hit-testing** — compare pointer focus, cursor and window geometry against a single scaled coordinate space for two tested framebuffer sizes.
- [ ] 540. **Interactive desktop endurance proof** — run actual Terminal, Files and editor ELFs for an extended QEMU session with input, resize and close/reopen operations; attach screenshot and leak counters.

### C. Durable block filesystem, transaction recovery and quotas (P0)

Deliver power-loss-safe *device-backed* data paths; do not treat RAMFS transformations as persistent storage.

- [ ] 541. **Disk transaction ID allocation** — allocate a strictly increasing transaction generation that remains unambiguous across metadata mirror recovery.
- [ ] 542. **Journal record header validation** — validate magic, version, length and checksum before interpreting redo/undo bytes from an untrusted disk sector.
- [ ] 543. **Journal capacity preflight** — refuse a mutation that cannot reserve all transaction records instead of exposing partially committed directory changes.
- [ ] 544. **Write-ahead data ordering** — prove journal intent reaches the device before the corresponding inode/directory blocks through observed flush barriers.
- [ ] 545. **Journal commit marker atomicity** — publish a checksum-protected commit marker with one unambiguous recovery decision after interrupted writes.
- [ ] 546. **Uncommitted allocation reclamation** — recover block allocations from a transaction lacking the final commit marker without leaking free space.
- [ ] 547. **Replay ordering across independent files** — recover two interleaved transactions in committed generation order while preserving each file's final bytes.
- [ ] 548. **Inode deletion tombstones** — prevent stale directory handles from reopening an inode that was deleted and later reused with the same numeric slot.
- [ ] 549. **Metadata mirror disagreement diagnostics** — distinguish stale, torn and cryptographically untrusted/invalid mirror copies without destroying the last known-good image.
- [ ] 550. **Directory entry checksum boundaries** — detect truncated or corrupted path names and reject a malformed record before allocating dependent inodes.
- [ ] 551. **File size overflow enforcement** — reject seek/write arithmetic that overflows storage coordinates or exceeds filesystem-defined maximum file size.
- [ ] 552. **Per-user logical-byte quota** — charge authenticated owner for new bytes before a write and release charges only after successful truncate/delete commit.
- [ ] 553. **Per-user inode quota** — reject unbounded empty-file creation, with accurate quota rollback on duplicate-name and partial device-write failures.
- [ ] 554. **Shared-directory quota attribution** — define which authenticated principal owns cost when a user moves a file between directories with different policies.
- [ ] 555. **Crash-safe truncate** — shrink or extend an existing file without exposing uninitialized blocks or losing the old committed content after power loss.
- [ ] 556. **Concurrent reader version pinning** — keep reads consistent while another task commits a replacement, with explicit old/new revision visibility.
- [ ] 557. **Block I/O cancellation semantics** — do not report a cancelled write as durable; handle in-flight IDE completion before freeing its DMA/bounce storage.
- [ ] 558. **Unaligned final-block write handling** — preserve unaffected bytes in a partially written final sector, including reboot validation.
- [ ] 559. **Filesystem repair dry-run** — generate an auditable, read-only list of recoverable and unrecoverable metadata issues without modifying the disk image.
- [ ] 560. **Filesystem endurance and recovery matrix** — execute at least 100 deterministic create/rename/append/truncate operations across injected resets and verify file digests after every clean reboot.

### D. Networking, socket ABI and headless service hosting (P0 after urgent M1 gates)

Integrate with verified R028 virtio-net demo but keep new general-purpose sockets and server services unclaimed until QEMU evidence. These are sequenced *after* urgent normal-boot and durable-storage gates.

- [ ] 561. **Versioned socket syscalls** — define the kernel/userland socket-create/bind/connect/send/recv/close ABI with strict structure size and feature-version validation.
- [ ] 562. **Socket rights by capability** — require authenticated rights for bind/listen/connect/send and reject use of a copied or stale socket handle from another PID generation.
- [ ] 563. **Per-process socket quota** — bound simultaneous sockets and pending receive bytes to avoid exhausting fixed kernel network buffers.
- [ ] 564. **Socket lifecycle cleanup** — close and reclaim receive queues, ephemeral ports and PCI-buffer references when a user process dies.
- [ ] 565. **Ephemeral local port allocator** — choose distinct bounded outbound ports and retire them safely on connection timeout and close.
- [ ] 566. **IPv4 route selection table** — use a bounded longest-prefix lookup with explicit interface and default-route validation instead of assuming one hardcoded peer.
- [ ] 567. **Neighbor cache entry expiration** — age ARP entries using monotonic deadlines and reject replies inconsistent with the active network interface.
- [ ] 568. **ARP request throttling** — avoid retry storms for unreachable peers and let the caller observe a bounded connection failure.
- [ ] 569. **DHCP lease renewal** — validate offer/ack options, renew an expiring lease and preserve a known-good address until a verified replacement is available.
- [ ] 570. **DHCP failure recovery** — fall back to a clearly reported unconfigured state without creating an invalid default route or sending packets as another host.
- [ ] 571. **DNS compressed-name parser** — bound pointer recursion and validate packet compression offsets to reject DNS cycles, overreads and oversized label chains.
- [ ] 572. **DNS timeout and retry policy** — retry limited queries with correct transaction IDs and report explicit NXDOMAIN, timeout and malformed-response errors.
- [ ] 573. **TCP receive sequence tracking** — reject out-of-window segments, duplicate payloads and inconsistent acknowledgment numbers before appending to an application receive buffer.
- [ ] 574. **TCP retransmission timer** — retransmit bounded unacknowledged segments with backoff and fail connections that exceed the retry budget.
- [ ] 575. **TCP close state machine** — support FIN/ACK, half-close and TIME_WAIT-style resource retirement without immediately reusing unsafe socket identity.
- [ ] 576. **Bounded inbound SYN queue** — enforce listen backlog limits and prevent half-open peers from exhausting all network memory.
- [ ] 577. **TCP send backpressure** — block or return WOULD_BLOCK at the documented buffer limit without silently dropping application bytes.
- [ ] 578. **Concurrent HTTP request/reply demo** — handle two independent ring-3 server client sessions without cross-socket buffer leakage in QEMU.
- [ ] 579. **Server profile network allowlist** — require explicit authorization to expose a listener beyond loopback and record the enabled port/service identity.
- [ ] 580. **Real network integration suite** — exercise DHCP/DNS/TCP concurrent sockets over virtio-net with host packet captures, timeouts and process-teardown assertions at the exact PR SHA.

### E. Secure update, recovery chain and CI gate evidence (P0 groundwork)

Security and recovery tasks must preserve signed-boot expectations, native QEMU proof, and strict branch-protection policy. Installation scope is groundwork, not a production release claim.

- [ ] 581. **Boot component digest manifest** — pin bootloader, kernel ELF and initrd hashes in a versioned artifact record consumed by reproducible build verification.
- [ ] 582. **Signed manifest trust-store loading** — validate a built-in public-key trust root before accepting metadata for privileged system updates.
- [ ] 583. **Signature algorithm allowlist** — reject unknown, downgraded or ambiguously encoded signature algorithms and unsupported public-key formats.
- [ ] 584. **Update monotonic rollback counter** — protect boot-critical version transitions against replaying an older signed image after a newer release was accepted.
- [ ] 585. **A/B boot slot staging design** — define bounded ownership and hash checks for two alternate system images without allowing updates to overwrite the current working slot.
- [ ] 586. **Inactive-slot write permission** — restrict install writes to the explicitly selected inactive slot and deny requests targeting recovery or firmware extents.
- [ ] 587. **Update commit marker recovery** — leave the previous bootable slot active until its replacement image and metadata are fully verified and flushed.
- [ ] 588. **Watchdog fallback after bad update** — detect repeated failure of the newly selected boot image and return to the last known-good slot without destroying user files.
- [ ] 589. **User data preservation across update** — isolate OS image replacement from user/data partition metadata with preflight partition-boundary assertions.
- [ ] 590. **Recovery signature negative tests** — modify one signed update byte and prove the boot/update path rejects it without changing the active kernel image.
- [ ] 591. **Recovery-mode package whitelist** — permit only explicitly signed recovery diagnostics while the system volume is read-only or degraded.
- [ ] 592. **Audit trail for privilege changes** — emit bounded records for capability grants, revoked handles, privileged file writes and service restart decisions.
- [ ] 593. **Secret-bearing log redaction** — prove guest serial/panic traces omit tokens, raw credential buffers and file contents even under injected authorization faults.
- [ ] 594. **Tamper-evident recovery metadata** — bind recorded boot image identity and recovery decisions to validated integrity metadata, not merely editable text markers.
- [ ] 595. **Operator recovery confirmation contract** — require explicit user consent before any potentially destructive repair or reinstall step touches a real filesystem.
- [ ] 596. **Read-only recovery shell quality gate** — expose bounded status, disk identity, error reason and safe file read operations while mutation APIs remain denied.
- [ ] 597. **Signed package activation atomicity** — verify package payload and dependency state before switching the visible on-device app version; roll back without orphaned permissions.
- [ ] 598. **Device-backed package receipt record** — persist installed package hash/version and source identity atomically for later audit and rollback decisions.
- [ ] 599. **CI malicious-update corpus** — replay wrong signer, stale version, corrupt boot slot, interrupted flush and forged manifest inputs on disposable QEMU images.
- [ ] 600. **M1-to-M2 release readiness review** — maintain signed-off evidence for every original M1 gate, including QEMU boot, desktop, durable reboot, syscalls and recovery; block graduation on missing proof.

### Acceptance / implementation rules for #501–600

- Check each item only after **production implementation**, targeted regression tests and integrated **exact-commit-SHA QEMU** or device evidence appropriate to the requirement. A checklist entry, API stub, mock screenshot or host-only test cannot close a kernel gate.
- Don't repeat accepted existing work or duplicate unfinished #001–500 tasks in the official M1 count: these are independently verifiable refinements/dependencies, **not** automatically 100 extra functional OS features.
- Keep the existing single-source-commit branch / PR #732 structure; do not add an unrelated PR, fabricate running agents or bypass GitHub branch protection.
- Leave draft until host Rust, no_std/security, ELF/ISO boot, QEMU runtime, storage/fault recovery, review and SHA validation pass for **this exact new SHA**.


## R085 extension VII — another 100 **PLANNED** M1 engineering subtasks (#601–700)

**Status: all #601–700 remain unchecked.** This request adds actionable acceptance and implementation planning to the existing PR, **not** 100 implemented Rust changes, active AI coding agents, green workflows, or completed M1 gates. The canonical `docs/IMPLEMENTATION_BACKLOG.md` / M1 gate tracker takes precedence when prioritizing actual code.

**Sequence:** address confirmed P0 blockers in normal PID1 boot, ring-3 compositor and persistent Files/Terminal first. The PCI and release-engineering tasks are dependency work, not permission to postpone core boot acceptance. All five workstreams are logical ownership groupings rather than five independent agents.

**Accounting before this extension:** 600 numbered items, 226 source-implemented/checkmarked, 374 unchecked. **After this documentation-only update:** 700 numbered items, 226 source-implemented/checkmarked, 474 unchecked. Mixed-granularity item counts are **not** percentages of functional IndianOS completion.


### A. PID1 startup contracts, service supervision and kernel isolation (P0)

Refines existing M1 boot/scheduler gates. All tests must exercise the real service path, not synthetic service-graph helpers.

- [ ] 601. **Boot-to-PID1 contract handshake** — pass a versioned boot context with verified memory, command-line and platform handles into the first user process; reject missing or stale fields in a negative QEMU boot.
- [ ] 602. **PID1 startup barrier** — prevent desktop and nonessential background services from launching before the filesystem, IPC and device managers have announced authenticated readiness.
- [ ] 603. **Boot profile command-line trust** — accept normal, headless and recovery profiles only from approved kernel/boot metadata, not an arbitrary unprivileged argument.
- [ ] 604. **Service binary image ownership** — pin each loaded executable inode/revision through mapping so a concurrent updater cannot change bytes while the child starts.
- [ ] 605. **Mandatory service launch rollback** — after an ELF load or capability-transfer failure, unwind reserved frames, handles, scheduler slots and supervisor records as one failed attempt.
- [ ] 606. **Service stop request authorization** — require exact authority to stop a child and reject a same-PID but different-generation caller even if it knows the numeric service ID.
- [ ] 607. **Graceful stop deadline** — deliver a cooperative stop signal, enforce a bounded deadline, and force cleanup without deadlocking the scheduler if the child ignores it.
- [ ] 608. **Service dependency version mismatch** — deny a child whose required IPC protocol version is incompatible with the ready provider; identify both versions in serial diagnostics.
- [ ] 609. **Service health pulse integrity** — permit only the current registered child generation to update its health status and expire the record after scheduler-confirmed exit.
- [ ] 610. **Misbehaving service CPU containment** — cap repeated busy loops or over-budget runtime of an optional service without blocking PID1 or the recovery shell.
- [ ] 611. **Recovery supervisor isolation** — start recovery services under restricted capabilities and prove they cannot mutate boot images or bypass read-only data-volume policy.
- [ ] 612. **Essential service restart dependency order** — stop dependents, restart the essential provider, and then restart only still-enabled dependents after authenticated READY.
- [ ] 613. **Restart history monotonicity** — assign unique attempt identifiers and retain ordered boot-local failure outcomes through rapid crash/restart loops.
- [ ] 614. **Service log channel backpressure** — bound each child UART/log relay and report dropped bytes without allowing a chatty app to block the kernel logging service.
- [ ] 615. **Ready-without-running rejection** — reject a forged READY event when no matching live scheduler task exists, even if a service manifest slot remains allocated.
- [ ] 616. **Active user session ownership** — bind desktop service handles to the authenticated login/session generation and revoke all descendants on logout.
- [ ] 617. **Supervisor persistence boundary** — distinguish boot-local process state from durable configuration; never persist temporary PIDs or capability handles across reboots.
- [ ] 618. **Service-degraded boot outcome** — explicitly expose which nonessential service failed while keeping Terminal and read-only diagnostic tools usable.
- [ ] 619. **No-automatic-restart-on-security-violation** — quarantine a child terminated for permission escalation rather than endlessly respawning the hostile binary.
- [ ] 620. **End-to-end service dependency proof** — run a three-level real ring-3 service dependency chain, intentionally fail the middle process, and retain QEMU evidence of ordered teardown and recovery.

### B. Real text rendering, desktop interaction and user session UX (P0)

Targets an observable Terminal/Files/editor session running under protected ring-3 clients and real input devices.

- [ ] 621. **Framebuffer glyph atlas bounds** — validate loaded glyph dimensions, Unicode codepoint index and backing byte length before rasterizing any user-facing text.
- [ ] 622. **Console Unicode replacement behavior** — render invalid byte sequences as a deterministic replacement glyph without overrunning the text rasterizer.
- [ ] 623. **Terminal grapheme-aware backspace** — delete one visible grapheme cluster instead of one byte when editing a UTF-8 command, preserving cursor coordinates.
- [ ] 624. **Terminal wide-glyph placement** — render full-width characters without splitting them across the last column or corrupting the next line.
- [ ] 625. **Terminal scrollback memory quota** — cap retained lines by bytes and free historical storage when clients close or resize the terminal.
- [ ] 626. **Terminal command cancellation** — map Ctrl+C to a permitted foreground child cancellation without killing PID1 or the terminal window itself.
- [ ] 627. **Terminal pipe input backpressure** — process a producer faster than the consumer with bounded buffers and no permanent hang after one endpoint exits.
- [ ] 628. **Terminal stderr/stdout separation** — expose both streams from real child descriptors and preserve correct exit status when one stream closes early.
- [ ] 629. **Terminal quoted argument parser** — correctly tokenize escaped spaces, unmatched quotes and repeated whitespace with explicit command error results.
- [ ] 630. **Terminal history privacy boundary** — store history under the current user and deny it to other ring-3 identities after session changes.
- [ ] 631. **Filesystem icon metadata source** — derive the Files window icon/type display from actual inode type and bounded extension mapping, not filename spoofing alone.
- [ ] 632. **Files large-directory virtual scrolling** — render a bounded visible window of thousands of entries using real directory cursors without duplicating all records in RAM.
- [ ] 633. **Files breadcrumb path safety** — map every breadcrumb navigation action to a validated canonical directory handle instead of trusting a UI-provided path string.
- [ ] 634. **Files sorting stability** — sort by name/size/mtime with stable tie-breaking and no stale pointer reuse when entries change mid-refresh.
- [ ] 635. **Files hidden-file toggle** — switch visibility without altering actual permissions, file metadata or directory ordering outside the current view.
- [ ] 636. **Editor long-line viewport** — horizontal-scroll large lines with bounded render work and exact caret-to-byte mapping after edits.
- [ ] 637. **Editor atomic save feedback** — show durable success only after the VFS flush/rename boundary acknowledges the saved revision.
- [ ] 638. **Desktop launch feedback** — reflect child ELF startup, READY, error and exit states in the taskbar without fake progress indicators.
- [ ] 639. **Keyboard-only accessibility route** — navigate window switcher, focus, menu, dialog and file list solely with physical keyboard events in a QEMU test.
- [ ] 640. **Multi-resolution desktop acceptance** — prove the same protected two-app drawing and mouse focus sequence at two supported QEMU framebuffer sizes with genuine screenshots.

### C. Persistent hierarchical VFS, device failure and file identity (P0)

Refines device-backed filesystem and cross-process file access gates; requires reboot and corruption evidence where relevant.

- [ ] 641. **Inode generation on reuse** — increment a persistent generation when a deleted inode slot is reassigned, rejecting handles from the previous occupant.
- [ ] 642. **Directory entry atomic publication** — prevent a reader from seeing a filename before its inode and block allocation metadata have committed.
- [ ] 643. **Directory name normalization contract** — specify case sensitivity and accepted byte encoding; prove duplicate-name and confusable-name policy on actual disk records.
- [ ] 644. **Root mount permission snapshot** — initialize `/`, `/home`, `/system` and `/recovery` ownership from verified mount metadata before userland opens files.
- [ ] 645. **Readonly remount state transfer** — quiesce writes, flush acknowledged transactions and switch the mounted volume into a read-only state without reopening old writable handles.
- [ ] 646. **Read-write remount authorization** — permit recovery mode to restore writes only after integrity scan and an explicitly privileged transition.
- [ ] 647. **Open file across directory rename** — preserve a valid open inode handle while the parent directory moves, with correct path resolution for new lookups.
- [ ] 648. **Open-unlinked file reclamation** — keep data readable through a live descriptor after unlink and reclaim disk blocks only when all descriptor leases close.
- [ ] 649. **Directory rename into descendant denial** — reject moving a directory inside its own subtree atomically, including after canonical path normalization.
- [ ] 650. **Recovery replay invalid-parent check** — refuse an otherwise checksum-valid journal entry whose parent inode is absent or points to an incompatible object type.
- [ ] 651. **Metadata generation rollover policy** — implement a safe, unambiguous strategy before transaction counter exhaustion, with tests at the final valid generation.
- [ ] 652. **Block allocator reserved-space threshold** — preserve capacity needed for critical journal metadata and recovery records even when a user fills the data volume.
- [ ] 653. **Per-file allocation map consistency** — cross-check extents against block ownership maps and quarantine overlaps without modifying unrelated inodes.
- [ ] 654. **File growth sparse-gap proof** — verify a written offset beyond EOF exposes deterministic zero bytes or returns a documented unsupported error, never stale sector data.
- [ ] 655. **Access time mutation policy** — define whether reads update timestamps and enforce its quota/journal implications instead of silently writing on read-only mounts.
- [ ] 656. **Authenticated file lock ownership** — bind advisory/exclusive locks to process generation and release them exactly once after crash or descriptor close.
- [ ] 657. **Cross-handle write conflict** — detect and serialize overlapping writes from distinct processes so no successful write yields torn mixed bytes after reboot.
- [ ] 658. **Device media identity change** — refuse to keep using cached filesystem metadata after the emulated storage device serial or UUID changes mid-session.
- [ ] 659. **Journal checkpoint truncation** — reclaim only fully applied journal records after a durable checkpoint; prove replay still recovers interrupted checkpoints.
- [ ] 660. **Real hierarchical file lifecycle proof** — QEMU-create nested directories and files, rename parent directories, crash during replacement, then cold-boot and byte-verify all survivors.

### D. PCI driver lifecycle, IRQ/DMA safety and secure device boundaries (P0/P1)

QEMU-first driver work; do not promise unsupported broad physical hardware compatibility.

- [ ] 661. **PCI function discovery bounds** — validate bus/device/function enumeration limits and stop scanning malformed multifunction headers without invalid config access.
- [ ] 662. **PCI BAR size arithmetic** — reject zero, unaligned, wrapped and conflicting BAR address/size windows before mapping any MMIO space.
- [ ] 663. **PCI resource reservation ownership** — enforce exclusive driver claims on MMIO and I/O-port ranges and release them on failed driver probe.
- [ ] 664. **Driver probe failure rollback** — unwind claimed BARs, IRQs, frames and bus-master settings if device initialization aborts at any step.
- [ ] 665. **Bus-master enable authorization** — enable PCI bus mastering only after establishing a validated DMA target and ownership policy.
- [ ] 666. **DMA buffer physical-contiguity preflight** — reject unsupported scatter/gather layouts before device submission rather than assuming virtual contiguity.
- [ ] 667. **DMA bounce-buffer zeroing** — clear newly allocated and returned bounce-buffer contents before transferring ownership between processes or devices.
- [ ] 668. **DMA completion ownership handoff** — forbid task memory reuse until an in-flight device descriptor is acknowledged or safely cancelled.
- [ ] 669. **IRQ vector exhaustion policy** — fail device interrupt registration cleanly when vectors are exhausted, keeping already bound devices functional.
- [ ] 670. **Interrupt storm quarantine** — detect a virtual device that triggers excessive interrupts and isolate its vector without silencing global timer interrupts.
- [ ] 671. **Level-triggered IRQ deassertion** — acknowledge and unmask an interrupt only after device-specific pending state has been cleared.
- [ ] 672. **Device reset generation lease** — invalidate stale command completions and handles after a controller reset and require re-probing under a new generation.
- [ ] 673. **IRQ handler allocation prohibition** — audit interrupt handlers to ensure no blocking allocation, filesystem I/O or sleeping lock occurs within interrupt context.
- [ ] 674. **USB host controller discovery policy** — enumerate potential xHCI functions safely but keep them disabled until IRQ, DMA and ownership boundaries are implemented.
- [ ] 675. **Multiple-block-device enumeration** — distinguish data, boot and disposable test disks by validated identity and prevent a driver from automatically selecting a destructive target.
- [ ] 676. **Storage request queue fairness** — prevent a long sequence of large reads from starving small metadata/journal flush operations under device pressure.
- [ ] 677. **Disk error propagation from IRQ path** — translate hardware timeouts and status flags into typed asynchronous completion errors without claiming a successful flush.
- [ ] 678. **APIC/PIC mode transition evidence** — instrument IRQ ownership during controller handoff and prove timer and PS/2 vectors remain live in a QEMU regression.
- [ ] 679. **Driver detach under pending I/O** — test unplug/reset while reads and writes are outstanding; complete or cancel all waiters and release device resources once.
- [ ] 680. **Device isolation stress acceptance** — trigger concurrent disk, virtio-net, timer and PS/2 traffic in QEMU; require no unowned DMA, IRQ lockup or user-space leakage.

### E. Security verification, reproducibility and CI gate enforcement (P0)

Exact-SHA acceptance and artifact integrity, preserving priority of already-unfinished kernel and M1 gates.

- [ ] 681. **CI job matrix canonical manifest** — declare required normal boot, recovery, host/no_std, filesystem, driver and security gates in one version-controlled source of truth.
- [ ] 682. **Required job removal detection** — fail the verification coordinator if a mandatory workflow is deleted, renamed or made optional without an approved gate change.
- [ ] 683. **Workflow permissions least privilege** — ensure default GitHub Actions jobs have read-only repo access, granting write permissions only to justified trusted steps.
- [ ] 684. **Untrusted PR execution boundary** — prevent fork-origin inputs from executing with write-capable tokens or exposing protected workflow credentials.
- [ ] 685. **Dependency artifact hash pinning** — verify externally fetched build tools, firmware and test images against approved digests before guest execution.
- [ ] 686. **Build input environment inventory** — record environment variables that affect source compilation while redacting secrets and volatile host-specific paths.
- [ ] 687. **Reproducible kernel binary comparison** — rebuild the same SHA twice in independent environments and investigate any ELF/ISO digest differences.
- [ ] 688. **Host regression test inventory audit** — enumerate unit tests by source module and fail when an expected test suite is silently filtered out.
- [ ] 689. **QEMU monitor operation restrictions** — prohibit the test harness from injecting fabricated guest success lines or modifying in-guest files behind the kernel.
- [ ] 690. **Guest serial assertions against branch SHA** — require a kernel-emitted build identity matching the source SHA and reject logs from a previous run.
- [ ] 691. **Exit-code propagation integrity** — fail the workflow whenever QEMU, cargo, security suite or artifact verification exits nonzero, even if a later reporting step succeeds.
- [ ] 692. **Stalled guest classification** — distinguish boot-timeout, serial inactivity, CPU lockup and test assertion failure with bounded teardown and retained diagnostics.
- [ ] 693. **CI artifact retention policy** — keep serial traces, QEMU screenshots, corrupted disk snapshots and crash logs for a declared diagnostic period without exposing secrets.
- [ ] 694. **Disk-image write provenance attestation** — record snapshot image ID and guest block write/flush trace so reboot persistence claims are traceable to actual I/O.
- [ ] 695. **QEMU cross-profile security suite** — replay the same stale-handle, cross-PID and unauthorized-write probes in normal, recovery and headless startup profiles.
- [ ] 696. **Kernel fault injection coverage register** — associate each vulnerable subsystem with at least one deterministic failure point and expected recovery outcome.
- [ ] 697. **Boot regression triage priority** — prevent feature batches from merging while any mandatory serial-boot, normal PID1 or disk-recovery regression remains unresolved.
- [ ] 698. **Integration PR one-commit invariant** — assert the PR is exactly one commit ahead of the recorded main base and reject accidental unrelated commit stacking.
- [ ] 699. **Acceptance-evidence traceability validator** — require a task marked done to reference real code changes, test case, exact-sha job run and any mandatory QEMU artifact.
- [ ] 700. **M1 completion board reconciliation** — reconcile the canonical gate tracker and 700-item checklist; avoid claiming percentages based on helper methods or planned subtasks.

### Completion contract for #601–700

- Every task is initially **not implemented**; check a box only after it has a traceable production change, regression test and relevant real QEMU/guest evidence tied to the same commit SHA.
- Treat detailed subtasks as refinements of the established M1 gate backlog; avoid double-counting overlapping acceptance criteria as separate completed OS capabilities.
- Preserve the one-source-commit policy for PR #732. Do not push new feature branches/PRs as part of this checklist update, claim five independently running agents, or hide test failures.
- Require all mandatory no_std, security, ELF/ISO boot, normal guest runtime, storage-recovery and review checks to pass for the **new exact head SHA** before any merge.


## R085 extension VIII — 100 new **planned implementation and acceptance tasks** (#701–800)

**All #701–800 are PLANNED / unchecked.** This extension creates actionable, code-and-QEMU-testable work items; it does **not** add 100 implementations, new agents, green CI, actual QEMU evidence, or finished M1 gates. The M1 gate tracker #616 and `docs/IMPLEMENTATION_BACKLOG.md` remain the primary source of truth. Tasks here refine already-open subsystem integration and must not be double counted as 100 unrelated features.

**Previous state:** 700 numbered entries: 226 source-implemented/checkmarked (not all CI validated) and 474 unchecked. **Updated:** 800 unique entries: 226 source-implemented/checkmarked and 574 unchecked. Five workstreams are logical task ownership, not proof of five simultaneously working AI agents. This change is documentation-only.


### A. Normal boot orchestration: device discovery → PID1 → services (P0)

Integration tasks only. Acceptance must follow ordinary normal-profile QEMU boots, not synthetic unit-test state.

- [ ] 701. **BootInfo handoff CRC field** — checksum the versioned boot-context header and reject a corrupted handoff before user address spaces are created.
- [ ] 702. **Boot graph feature-mask compatibility** — reject a service demanding unavailable kernel capabilities before attempting its ELF load; log the exact mask.
- [ ] 703. **Mandatory dependency closure check** — compute the transitive required-service closure and reject startup when any indirect dependency is omitted from the signed manifest.
- [ ] 704. **Bootstrap service capability minting** — generate narrowly scoped root IPC/VFS handles for PID1 without granting write access to boot/recovery partitions.
- [ ] 705. **Boot-volume-to-service readiness barrier** — wait for a verified mounted data-volume identity before launching userland that requires persistent state.
- [ ] 706. **Keyboard-availability fallback at boot** — expose an alternate serial recovery console when no PS/2 controller is detected, without blocking headless startup.
- [ ] 707. **GPU/framebuffer optional dependency policy** — start non-graphical system services when graphics initialization fails and report desktop unavailability explicitly.
- [ ] 708. **Serial shell handoff ownership** — transfer the active diagnostic console between bootstrap and PID1 exactly once, preserving the final kernel message ordering.
- [ ] 709. **Authenticated service HELLO handshake** — validate protocol, parent service ID, challenge and current PID generation before granting the child's mailbox handle.
- [ ] 710. **Boot attempt cancellation barrier** — abort a partially initialized child on recovery transition before the scheduler makes it runnable.
- [ ] 711. **Service readiness persistence ban** — reject any attempt to restore READY from the prior boot image; every new boot must perform a fresh handshake.
- [ ] 712. **Supervision event ordering invariant** — serialize STARTING, READY, FAILED and EXITED events so consumers never observe READY after final exit.
- [ ] 713. **Dependency stop propagation breadth test** — terminate one provider shared by three dependents and prove no dependent keeps an invalid service handle.
- [ ] 714. **Optional service failure retry window** — reset retry limits only after a documented healthy interval, not immediately after a short-lived READY.
- [ ] 715. **Service startup resource envelope** — preflight combined stack, page table and mailbox reservations across all concurrently scheduled startup children.
- [ ] 716. **PID1 self-watch failure classification** — distinguish PID1 exception, process exit and kernel panic so only safe failures select a recovery profile.
- [ ] 717. **Kernel userland boot-complete marker** — emit boot success only when mandatory real ring-3 service acknowledgments and device mount guards are satisfied.
- [ ] 718. **Post-boot supervisor reload** — stage manifest changes, validate graph and apply changes without terminating unrelated, currently healthy service processes.
- [ ] 719. **Graceful shutdown service deadline graph** — stop children before providers, flush persistent services and preserve a serial reason when a deadline is exceeded.
- [ ] 720. **Ten-boot ordinary-profile invariant sweep** — record process/service generations and memory counters over ten cold boots with zero leaked startup objects.

### B. User-space compositor: real surface protocol, input and desktop reliability (P0)

Each task must reach a protected ring-3 client and the physical QEMU framebuffer. Kernel-owned render demos alone do not pass.

- [ ] 721. **Compositor protocol capability handshake** — negotiate a per-client surface ABI and reject unsupported formats before sharing framebuffer memory.
- [ ] 722. **Surface stride/height byte-budget audit** — validate stride × height using checked arithmetic against the exact owned shared-memory extent.
- [ ] 723. **Per-surface acquire/release fence** — ensure a client cannot update a buffer while the compositor presents it; detect a stale fence generation.
- [ ] 724. **Compositor input event serial numbering** — use monotonically ordered per-seat sequence IDs so duplicate keyboard/mouse deliveries can be rejected.
- [ ] 725. **Keyboard-to-window routing transaction** — atomically publish new focus and cancel queued input for the previous window before forwarding the next scancode.
- [ ] 726. **Mouse button edge preservation** — retain press and release transitions across movement coalescing, including a full pointer-event queue.
- [ ] 727. **Pointer capture cancellation on unmap** — release drag/capture if its originating surface memory is revoked while a mouse button remains held.
- [ ] 728. **Damage-rectangle union saturation** — coalesce excessive dirty rectangles into one safe full-surface refresh instead of discarding content updates.
- [ ] 729. **Client frame acknowledgment deadline** — mark a nonresponsive client's surface stale without freezing other windows or blocking PID1.
- [ ] 730. **Actual z-order repaint proof** — overlap three independent ELFs, reorder them and compare captured pixels against expected visible regions.
- [ ] 731. **Transient window owner inheritance** — bind dialogs to a parent process generation and automatically close them when the parent crashes.
- [ ] 732. **Compositor crash-safe cursor reset** — move the physical pointer into a visible valid screen position after compositor restart without reusing stale capture.
- [ ] 733. **Shared-surface mapping quota rollback** — enforce per-client page quotas and free all partially reserved mappings if surface creation fails.
- [ ] 734. **Client render packet fuzz isolation** — feed malformed operation counts, coordinates and user pointers from ring-3 and prove no out-of-bounds compositor read.
- [ ] 735. **Terminal repaint after screen wake** — trigger a full redraw on framebuffer reinitialization while preserving current terminal document and caret.
- [ ] 736. **Files window refresh after external mutation** — invalidate its directory snapshot on an authenticated VFS mutation event and redraw only affected entries.
- [ ] 737. **Window switcher priority contract** — ensure secure system dialogs take precedence over client-requested focus and prevent focus-stealing by background ELFs.
- [ ] 738. **Compositor logout and seat teardown** — revoke clipboard, window and keyboard queues before activating a different user session generation.
- [ ] 739. **Frame latency instrumented budget** — measure input-to-frame latency under sustained file I/O with QEMU traces and document the observed distribution.
- [ ] 740. **Desktop recovery runbook test** — kill compositor during an editor save, restart it, remap only valid surfaces and show no loss of committed file bytes.

### C. Terminal and Files through durable hierarchy: crash and reboot semantics (P0)

No RAM-only shortcuts. Require mounted device-backed VFS semantics, userland calls and cold-reboot evidence.

- [ ] 741. **Terminal cwd inode identity** — track the working directory by authorized directory handle rather than a mutable display path after parent rename.
- [ ] 742. **Terminal redirection with atomic file create** — implement stdout-to-file with checked create/replace rights and no truncated destination on failed command startup.
- [ ] 743. **Terminal append redirection durability** — append command output as ordered records and explicitly flush before reporting persisted success.
- [ ] 744. **Terminal pipeline process reaping** — close unused pipe ends and reap every subprocess when an upstream command exits early.
- [ ] 745. **Files file-create conflict token** — obtain a current directory generation, perform conditional create and surface a deterministic conflict on concurrent insertion.
- [ ] 746. **Files directory-delete rollback** — refuse deletion of a nonempty or concurrently changed directory without removing any child inode.
- [ ] 747. **Files copy verify-then-publish** — stream large files through bounded RAM, checksum copied content and publish the destination only after durable verification.
- [ ] 748. **Files rename across nested directories** — verify source/destination parent ACLs and update both directory generations in a single journal transaction.
- [ ] 749. **Files multiple-selection operation preflight** — validate all selected inode identities and permissions before beginning a multi-file rename/delete plan.
- [ ] 750. **Files interrupted transfer resume policy** — either persist a verifiable transfer checkpoint or delete the uncommitted partial file after a crash.
- [ ] 751. **Editor save-as destination isolation** — validate the new path and directory rights, then atomically publish bytes without modifying the original file.
- [ ] 752. **Editor undo buffer quota** — cap memory consumed by undo history, report truncation and never substitute stale recovered content for current disk data.
- [ ] 753. **Editor autosave generation lease** — prevent a delayed autosave from overwriting a newer manually saved revision or another client's edit.
- [ ] 754. **Durable workspace root bootstrap** — create authenticated home/workspace directories once with explicit ownership and retain them across QEMU cold boots.
- [ ] 755. **Cross-process file notification identity** — signal watchers using inode generations and prevent events for a newly reused numeric inode from leaking.
- [ ] 756. **Directory search cancellation** — stop a user-requested recursive search without leaving open cursors, orphaned worker tasks or blocked filesystem locks.
- [ ] 757. **Per-file integrity error propagation** — show a typed read error in Terminal/Files when a sector checksum fails, rather than silently displaying truncated success.
- [ ] 758. **Recovery mount file browsing** — allow safe userland read-only navigation after journal corruption while rejecting all create, rename and write operations.
- [ ] 759. **Cold-boot multi-app shared workspace** — create through Terminal, edit in editor, inspect in Files, reboot and compare exact file bytes and revision.
- [ ] 760. **Powercut-at-save publication sweep** — interrupt ten points between temp creation, data flush and rename; accept only complete old or new versions.

### D. Capability security, process lifetime and kernel synchronization (P0)

Test real syscall ingress and teardown races with multiple hostile ring-3 tasks; policy-only helper methods are insufficient.

- [ ] 761. **Opaque handle anti-guessing test** — enumerate arbitrary descriptor values from hostile ring-3 and prove none authorize another process's resources.
- [ ] 762. **Inherited capability manifest validation** — reject a child launch containing any right not granted by the parent or the trusted supervisor.
- [ ] 763. **Capability handle generation exhaustion** — fail closed before handle generations wrap into a value that could validate a retired reference.
- [ ] 764. **Process exit versus IPC dispatch fence** — prevent a sender from publishing after teardown begins even when it has already passed preliminary authorization.
- [ ] 765. **Mailbox drain ordering under exit** — atomically unlink a dead receiver, wake its senders and retire queued payload memory exactly once.
- [ ] 766. **User-copy TOCTOU snapshot** — validate and copy an entire syscall request before interpreting rights/paths, without rereading mutable ring-3 fields.
- [ ] 767. **Shared memory revocation shootdown** — invalidate stale translations when a shared-surface capability is revoked from a different task.
- [ ] 768. **Per-user syscall quota audit** — ensure separate live processes under one user principal share an appropriately enforced resource budget.
- [ ] 769. **Scheduler wait-token ABA protection** — bind blocked-task wakeup records to a generation identifier, rejecting callbacks targeting a reused slot.
- [ ] 770. **Interrupt-disabled section watchdog** — measure maximum time IRQs remain masked and report code paths that violate the timer responsiveness budget.
- [ ] 771. **Lock-free queue memory ordering review** — document and test acquire/release edges for any producer-consumer queues touched by IRQ and user context.
- [ ] 772. **Process-owned driver request cancellation** — cancel or finish pending device I/O before reclaiming the exiting task's pinned memory.
- [ ] 773. **Per-process logging channel teardown** — close records and release quota on exit without flushing untrusted bytes into a later process's diagnostic stream.
- [ ] 774. **Copy-out post-revocation semantics** — define whether a completed read may publish results after a capability is revoked and test the exact linearization point.
- [ ] 775. **Memory mapping alias permission denial** — reject one process mapping the same protected frame through a second handle with increased rights.
- [ ] 776. **Cross-task file lock abandonment** — release an exited task's inode locks without dropping locks owned by other PID generations.
- [ ] 777. **Thread creation limit in one address space** — enforce per-process thread limits and unwind half-constructed stacks and scheduler records on rejection.
- [ ] 778. **Waiter cancellation with nested timeouts** — wake a task once when a timed syscall and process teardown race, preserving its final error code.
- [ ] 779. **Concurrent system-call fuzz capture** — run two adversarial ELFs with seeded syscall races and retain the seed and serial trace for reproduction.
- [ ] 780. **Privilege-separation exit test** — crash an app during file write, IPC transfer and surface rendering; verify PID1, other apps and kernel mappings remain intact.

### E. QEMU integration acceptance, measurement and recovery evidence (P0)

Operational exit gates: reliable exact-SHA guest evidence and fault reproduction, not mere assertions that code compiles.

- [ ] 781. **Canonical M1 gate matrix mapping** — map every high-level M1 gate to a uniquely named CI assertion, source area, owner and artifact path.
- [ ] 782. **CI actual-kernel identity embed** — embed build SHA in the guest ELF and reject guest logs from an image built from another commit.
- [ ] 783. **QEMU boot-profile permutation suite** — run normal, recovery and headless profiles against the same binary with independently isolated writable disks.
- [ ] 784. **Kernel boot timing phase markers** — timestamp bootloader handoff, VMM ready, PID1 ready, data mount and desktop input readiness in guest logs.
- [ ] 785. **Service failure fault classification** — distinguish injected crash, timeout, invalid ELF and missing dependency with unique machine-parseable result codes.
- [ ] 786. **Workstation screenshot process correlation** — pair each framebuffer artifact with the real PID/generation and owning service evidence from guest serial logs.
- [ ] 787. **Filesystem cold-reboot harness guard** — assert each persistence test terminates QEMU and launches a fresh process using the same device image.
- [ ] 788. **Powercut write-order fault matrix** — inject failures before intent flush, after data flush and during metadata mirror update, verifying old/new atomicity.
- [ ] 789. **Repeated frame cleanup budget** — compare free-frame counts before and after 100 child launch/exit cycles, failing measurable memory leaks.
- [ ] 790. **Interrupt throughput saturation measurement** — measure timer, PS/2, block and network IRQ handling during a combined stress boot without fabricated interrupts.
- [ ] 791. **QEMU process isolation capture** — archive fault address, PID generation and return errno for deliberate hostile cross-process syscall attempts.
- [ ] 792. **Recovery session usability proof** — show interactive read-only status and file browsing after an intentionally corrupted data disk without mounting it writable.
- [ ] 793. **Serial command output expectation file** — store deterministic expected outputs for shell operations and reject missing, reordered or fake success lines.
- [ ] 794. **CI timeout parameter provenance** — record configured per-stage deadlines and fail a regression if a timeout is silently disabled or made unbounded.
- [ ] 795. **Driver failure smoke matrix** — boot separately with missing disk, invalid framebuffer, unknown PCI function and absent keyboard controller.
- [ ] 796. **Persistent data golden checksum manifest** — compare an on-disk directory tree against a pre-shutdown digest list after a separate QEMU restart.
- [ ] 797. **Release artifact image/ELF consistency** — verify every exported ISO contains the identical kernel ELF digest that passed freestanding and guest checks.
- [ ] 798. **Minimal kernel diagnostics after panic** — capture bounded last-event and trap frames, proving no untrusted secret buffer leaks to serial output.
- [ ] 799. **M1 regression suite performance baseline** — report CPU, RAM and disk activity for ordinary boot and flag meaningful regressions without claiming universal hardware targets.
- [ ] 800. **PR merge-blocking evidence reconciliation** — only authorize merging when mandatory checks on the exact PR SHA succeed and all claimed completed M1 gate links resolve.

### Explicit completion and CI rules

1. Each item is **unchecked until actual production code, negative tests and targeted real-guest evidence are reviewed**. No host-only test, mockup, design document or count of Rust helper methods establishes ring-3/physical-input/persistent-disk acceptance.
2. Sequence the existing P0 normal PID1, real compositor/input, cross-process VFS and durable file recovery gates before unrelated workstation additions. This numbered appendix refines the current M1 gate plan and does not replace issue #616.
3. Keep PR #732 on one consolidated source commit and do not create another PR for this list. Preserve previous source changes and checklists. Do not claim five independent agents were launched.
4. Leave the PR draft until all exact-head GitHub CI requirements, freestanding/no_std security regression, ELF/ISO serial boot, QEMU runtime and durable-recovery tests are passing. Never bypass branch protection or merge a pending/failed branch.


## R085 implementation slice: process teardown, focus routing, and protected file reads

**Scope:** real production Rust changes in existing modules, not checklist padding. This is an initial M1 implementation slice toward the 574 open tasks; it does NOT complete all 574, wire the real syscall dispatcher, prove a ring-3 compositor, or establish durable filesystem semantics. End-to-end acceptance checkboxes remain unchecked until the required real guest and CI evidence exists.

### Production source changes

- rust/kernel_core/src/batch5_session.rs — validate the complete dependency graph before selecting or confirming service readiness (partial policy for #008–009); drop **outgoing as well as incoming** queued events on authenticated PID exit (partial #030/#377); discard the previous focused window's queued key events on a real focus transition while preserving keys for unchanged focus (#050/#054); revoke queued keys and focus when a window is closed (#070 policy); add authenticated and atomic multi-recipient IPC batch submission and no-partial-consume mailbox reception (policy for #026/#040).
- rust/kernel_core/src/batch5_vfs.rs — add open_authorized, read_authorized and close_owned for the actual fixed-capacity read-only file descriptor adapter. Every authorized read rechecks FILE_READ capability, exact PID-generation and lifetime, so a stale descriptor cannot continue to use this guarded API after grant revocation (policy for #098/#463). Trusted low-level APIs still exist: this is NOT a global VFS ACL.

### Targeted host Rust unit tests added

1. dead_sender_messages_are_reaped_before_pid_reuse
2. focus_switch_discards_old_keys_without_touching_new_owner
3. valid_only_group_publish_and_short_output_never_partially_commit
4. invalid_dependency_prevents_any_root_launch_selection
5. unauthorized_open_does_not_consume_handle_or_mutate_backend
6. path_traversal_and_generation_lease_fail_without_publish

**Verification still required:** run the six host tests, all existing security regressions, freestanding no_std target, normal guest QEMU boot, and actual ring-3 sender-exit/file-read access checks. No currently unchecked task has been auto-marked as complete. Keep the PR draft and unmerged until mandatory exact-SHA GitHub checks pass. Five agent workstreams remain logical, not independently launched development processes.
