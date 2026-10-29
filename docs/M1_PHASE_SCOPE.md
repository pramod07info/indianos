# M1 integrated phase backlog — full mandatory scope

**Release target:** graphical desktop boots without test flags, users can run independent ring-3 Files/Terminal, and editing a document is still visible after reboot on a qualified supported machine.
**Required phases:** 1–4, 8–10, with minimum driver support from 5–6. All tasks below are in the **one M1 workstream**, NOT claimed complete by this one commit.

**Single commit is a review/change packaging policy, not a promise that an entire OS can be implemented and independently hardware-tested in one commit.**

## Existing evidence that must not be discarded
- Rust-first kernel, Limine/OVMF, process and syscall/IPC experiments and QEMU APIC tests are already in main.
- QEMU opt-in R031.1 verifies automatically started **three** actual ring-3 desktop processes.
- QEMU opt-in R031.9 verifies graphical Files, real emulated disk, two boots, input and screenshots.
- R027/R031 durable COW metadata and bounded HierVfs exist. No autoformat of real hardware is allowed.
- M1 adds bounded read-only MBR/GPT partition table discovery (GPT header and entry-array CRCs), rejects overlap/out-of-bounds media and explicit partition selection, with sector-isolated mounting of the existing durable VFS. No disk is formatted by discovery or mount.
- New M1 modules connect Terminal `ls`, `cat`, `write`, `mkdir`, and `mv` to the same durable HierVfs, editable input line, verified process identity and per-window damage notifications. Feature-gated Rust tests exercise remount, failed writes and stale identities.
- The CI workflow now runs M1 host/no_std tests AND the existing genuine R031.9 QEMU disk+desktop integration, rather than treating the host adapter test as a ring-3 success.

## All phase tasks and required completion state
### P1 Bootloader and kernel startup

- [ ] x86-64 UEFI boot protocol — Existing bounded/experimental implementation; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Pinned Limine bootloader — Existing bounded/experimental implementation; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Kernel ELF loading and entry validation — Existing bounded/experimental implementation; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] UEFI memory map validation — Existing bounded/experimental implementation; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Serial logging/panic/framebuffer status — Existing bounded/experimental implementation; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Kernel stack and boot options — Partially implemented; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Validated initramfs boot modules — Existing bounded/experimental implementation; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Boot-failure/recovery mode — Not implemented or not production-qualified; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Verified/Secure Boot integration — Not implemented or not production-qualified; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Normal startup on selected physical UEFI PCs — Not implemented or not production-qualified; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.

### P2 CPU, interrupts and memory

- [ ] GDT/IDT/TSS exception handling — Existing bounded/experimental implementation; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Physical frame management and reclamation — Existing bounded/experimental implementation; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Virtual memory and protected mappings — Existing bounded/experimental implementation; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Kernel allocation/heap policies — Partially implemented; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] NX/W^X/guard page policies — Partially implemented; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Demand paging/COW/page faults — Partially implemented; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] High-resolution timer/APIC routing — Existing bounded/experimental implementation; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Multicore/SMP scheduler — Not implemented or not production-qualified; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Interrupt-safe atomics/locks/deadlock qualification — Partially implemented; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] FPU/SIMD and CPU topology context — Not implemented or not production-qualified; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Memory-pressure/OOM policies — Not implemented or not production-qualified; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Suspend/resume memory restoration — Not implemented or not production-qualified; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.

### P3 Processes, syscalls, scheduling and IPC

- [ ] Process and thread control blocks — Existing bounded/experimental implementation; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] User-mode protected address spaces — Existing bounded/experimental implementation; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Create/load/run/reap processes — Existing bounded/experimental implementation; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Preemptive multicore fairness and priorities — Partially implemented; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Versioned syscall ABI and safe user-copy — Existing bounded/experimental implementation; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Capabilities and authenticated handles — Existing bounded/experimental implementation; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Blocking/asynchronous IPC and shared memory — Partially implemented; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Signals and cancellation — Not implemented or not production-qualified; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Wait queues and futex-like facilities — Partially implemented; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Resource limits and thread synchronization — Partially implemented; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Parent-child service ownership — Partially implemented; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Fault recovery and adversarial stress acceptance — Partially implemented; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.

### P4 Durable filesystem and storage

- [ ] Block-device abstraction — Existing bounded/experimental implementation; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] GPT/MBR partition discovery — Partially implemented; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Hierarchical on-disk file/directories — Existing bounded/experimental implementation; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] VFS/mount/descriptor integration — Partially implemented; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Create/read/write/rename/delete/truncate — Partially implemented; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Persistent user ownership/ACLs and timestamps — Not implemented or not production-qualified; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Metadata journaling/COW transaction — Existing bounded/experimental implementation; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Page cache, fsync and flush semantics — Partially implemented; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Concurrent locking and file sharing — Not implemented or not production-qualified; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Quotas and general free-space allocation — Not implemented or not production-qualified; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Removable devices and filesystem encryption — Not implemented or not production-qualified; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Reboot/powercut/corrupt-metadata regression — Partially implemented; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.

### P5 Required bus and device framework

- [ ] ACPI tables and firmware inventory — Partially implemented; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] PCI enumeration on selected QEMU devices — Partially implemented; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] MMIO/PIO access validation — Existing bounded/experimental implementation; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] APIC/MSI interrupt resource routing — Partially implemented; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Bounce DMA and IOMMU enforcement — Partially implemented; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Driver probe/start/stop lifecycle — Partially implemented; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Driver crash isolation — Not implemented or not production-qualified; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Hotplug handling — Not implemented or not production-qualified; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Input/display/storage power and readiness — Partially implemented; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Diagnostic/error reporting — Partially implemented; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.

### P6 Minimum supported physical/emulated drivers

- [ ] Serial UART/console — Existing bounded/experimental implementation; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] PS/2 keyboard and mouse — Existing bounded/experimental implementation; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] UEFI-provided linear framebuffer — Existing bounded/experimental implementation; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Legacy ATA scratch disk driver — Existing bounded/experimental implementation; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] VirtIO network sample driver — Existing bounded/experimental implementation; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] AHCI/NVMe production driver — Not implemented or not production-qualified; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] USB/xHCI keyboard/mouse stack — Not implemented or not production-qualified; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Modern display/GPU devices — Not implemented or not production-qualified; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Hardware inventory across selected physical PCs — Not implemented or not production-qualified; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.

### P8 PID1, system services and sessions

- [ ] Normal boot into userspace without opt-in test flag — Not implemented or not production-qualified; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Authenticated PID1 service supervision — Partially implemented; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Dependency-ordered startup — Partially implemented; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Watchdog, restart and failover — Partially implemented; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Logging/config/settings services — Partially implemented; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Users/groups/session auth — Not implemented or not production-qualified; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Clock/timezone/locale management — Not implemented or not production-qualified; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Background jobs and notifications — Not implemented or not production-qualified; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Storage/input/package/network service daemons — Partially implemented; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Shutdown/reboot/logout and suspend lifecycle — Partially implemented; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.

### P9 GUI/window system

- [ ] Ring-3 compositor separate from kernel — Not implemented or not production-qualified; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Per-process private pixel surfaces — Partially implemented; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Window operations and stacking — Existing bounded/experimental implementation; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Keyboard focus/mouse delivery from physical IRQ — Partially implemented; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Desktop shell/taskbar/launcher — Partially implemented; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Unicode font/layout engine — Not implemented or not production-qualified; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Theme/icon/wallpaper controls — Partially implemented; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Graphics acceleration and software fallback — Not implemented or not production-qualified; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Multi-display/DPI scaling — Not implemented or not production-qualified; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Clipboard/drag-drop/notification UI — Not implemented or not production-qualified; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Compositor restart and sandbox isolation — Not implemented or not production-qualified; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] QEMU image/input acceptance and hardware displays — Partially implemented; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.

### P10 Essential desktop applications

- [ ] Graphical login and lock — Not implemented or not production-qualified; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Functional GUI Files CRUD explorer — Partially implemented; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Interactive graphical Terminal commands — Partially implemented; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Settings application GUI — Partially implemented; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Task Manager/monitor — Not implemented or not production-qualified; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Signed app installer GUI — Not implemented or not production-qualified; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Browser/PWA runtime — Not implemented or not production-qualified; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Text editor/document viewer — Not implemented or not production-qualified; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Media/image/audio apps — Not implemented or not production-qualified; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Network/display/sound settings — Not implemented or not production-qualified; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Clock/calculator/screenshot utilities — Not implemented or not production-qualified; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.
- [ ] Help, backup/recovery interface — Not implemented or not production-qualified; not accepted as a full M1 requirement until its documented QEMU/physical exit test passes.

## Exact M1 exit gate (ALL required, not achieved by this commit)
1. A *normal* installed OS boots via UEFI into a properly isolated ring-3 window server and two independent graphical applications.
2. A keyboard and pointer actually trigger Terminal and Files actions without COM1/host automation controlling the functionality.
3. In the guest, Files/Terminal create, read, edit, rename and delete documents on a durable disk with validated ownership/capabilities.
4. A shutdown and boot on the **same** scratch storage verifies the last committed file bytes; injected torn writes recover correctly.
5. A process crash and restart cannot reuse stale handles, credentials or graphics surface grants.
6. Selected supported devices pass CPU/memory/IRQ/input/display/storage diagnostics in QEMU and designated real PCs.
7. Code is independently reviewed; full exact-commit QEMU tests, host unit tests, no_std and real-hardware QA evidence all pass.

## Integration note
M1.1 is currently an actively evolving draft branch with one squashed feature commit, not all M1 completion. Subsequent work may be integrated into that same branch (replacing the single feature commit after checks are re-run), but only publish a release-complete claim after the above exit gate is independently satisfied.


### M1 G1/G2 selected-partition Terminal integration slice (2026-10-29)

The Rust \`m1_persistent_terminal::command_on_partition\` service bridges
a scheduler-authenticated process identity and the existing Terminal parser
to \`m1_partition::mount\`. The caller explicitly selects an MBR/GPT
partition ordinal, receives bounded filesystem errors, and uses only
\`BlockWindow\`-isolated checked COW I/O. Unlike the RAM-only R041 command
interpreter, every individual command mounts the selected real block volume
and rereads committed metadata. The service never formats a device.

Adversarial Rust regressions use two independently provisioned disk
partitions, write different contents at the same absolute VFS pathname,
rename and remount, verify untouched MBR/neighbor/gap sectors, reject stale
process generation, invalid partition index and corrupt media, and
simulate a failed COW sector write with committed-file recovery. These are
real \`BlockDevice\` sector operations in host tests, not a live QEMU
keyboard or true syscall proof.

**Status: G1 and G2 remain OPEN.** This kernel service does not yet connect
to actual ring-3 keyboard IRQ/syscall ingress, does not qualify physical
disks, and does not establish a GUI screenshot/reboot acceptance. Run
\`cargo test --offline --manifest-path rust/kernel_core/Cargo.toml
m1_selected_partition_\` plus all M1, no_std, security and QEMU CI against
the exact integration SHA. Never promote the milestone checkbox based on
host tests alone.


### R039 developer workstation M1 kernel-service slice (2026-10-29)

Actual Rust functions implemented: bounded owner-only DiskEditor undo/redo
including history invalidation and post-save dirty semantics; ProjectTree
literal UTF-8 replace-all using one generation-checked durable COW write;
and failure-safe desktop Terminal Enter retry that retains the command after
a Terminal-layer error but never replays after a post-commit redraw error.
Four new regression tests cover owner generation, bounds, no-op replacement,
remount persistence and undo/redo. Existing tests and QEMU checks remain
mandatory. This is service logic, not proof of live ring-3 IDE/compositor.
No claim of twenty concurrent coding agents or completion of M1 gates.


### R040 M1 workstation service integration (2026-10-29)

This batch changes three Rust production modules, the M1 CI acceptance
workflow, and this ledger. It is one consolidated commit; no placeholder
agent source files or 20 concurrently executing agents are claimed.

- Authenticated editor: direct DiskEditor save now protects OS, boot,
  firmware, recovery and key namespaces. Save As commits a separate file
  through a mounted COW HierVfs only if generation, identity, path and
  destination existence checks pass. Failed saves retain the user's dirty
  editor bytes and leave original disk files untouched.
- Desktop source editing: live PID generation and unexpired FILE_WRITE
  capability are mandatory for undo, redo, Save As, and UTF-8 project
  source replace-all. The replacement is a single-file COW transaction,
  not a multi-file atomic edit.
- Keyboard history: trusted focused PS/2 Up and Down arrow codes recall
  the last successfully executed Terminal command and restore a draft.
  History is scrubbed on exit, process fault or restart.
- Tests: seven new R040 regressions cover identities, protected paths,
  duplicate target, disk remount, bounds, keyboard history and expiry.
  The M1 CI workflow explicitly runs both existing m1_ tests and new
  r040_ tests; mandatory no_std, security and QEMU checks remain enabled.

M1 accepted milestone gates remain unchanged. The real normal-boot
ring-3 compositor and user Terminal, IRQ-to-disk filesystem read-after-
reboot, physical hardware and recovery acceptance remain open under
the M1-first directive in issue #616.


### R043 M1 workstation file toolkit, source editor and project line navigation (2026-10-29)

This is a M1-first, **single-commit integrated functional batch**. No fake
Agent 01..20 files and no assertion that the existing GitHub "ten modules"
verification workflow launched independently executing code-writing agents.

Actual production work:
1. Terminal "tools" command shows the supported file-tool subset.
2. Terminal count returns exact persisted file byte length (previously in help
   but unimplemented).
3. Terminal lines counts persisted UTF-8 source lines.
4. Terminal hash returns a 16-character FNV-1a diagnostic fingerprint
   (NOT a cryptographic integrity/authenticity mechanism).
5. Terminal stat returns file byte length and volume revision.
6. Terminal tail reads the last bounded N bytes, even for files larger than
   the terminal's output transfer.
7. Terminal find locates the first literal text match by byte offset.
8. Terminal grep returns complete matching lines without partial output.
9. Terminal cmp compares two complete persisted files.
10. Terminal touch creates an empty file only when absent, preserving existing
    data and enforcing reserved namespace denial.
11. Terminal copynew creates a new checked copy, failing rather than
    overwriting an existing destination.
12. Terminal replace performs full-file literal replacement with exactly one
    copy-on-write commit and no partial edits on oversize/bad paths.
13. Terminal appendln appends a line and LF via one durable file overwrite.
14. DiskEditor replace_text checks Unicode boundaries before local changes.
15. DiskEditor replace_all_text performs one undoable Unicode refactoring.
16. DiskEditor find_text navigates the in-memory unsaved source revision.
17. DiskEditor text_offset maps line/column to UTF-8 source offsets.
18. ProjectTree read_source_line reads a single Unicode source line atomically.
19. ProjectTree insert_source_line and delete_source_line commit durable
    bounded, per-file revision-locked line edits.
20. Desktop routes the new source editing, code search, file duplication,
    line navigation and line operations behind live PID-generation capability
    checks rather than trusting process-supplied identities.

Verification: 12 focused R043 regression tests added across Terminal,
DiskEditor, ProjectTree and Desktop. The mandatory M1 workflow runs the new
r043_ group in addition to prior M1 and R040 tests, no_std, kernel S0,
full kernel boot and actual graphical Files/disk persistence QEMU coverage.
All test and QEMU results must be verified by exact-head GitHub Actions
before any merge. Source test services do NOT alone close the eight outstanding
M1 acceptance gates (real normal-boot ring-3 UI/IRQ/storage/recovery proof).


### R044 M1: live Desktop Terminal cursor editor, protected history and safe input routing (2026-10-29)

The previous InputLine was append-only even though LineEditor supported
cursor operations in isolated tests. R044 wires the real bounded LineEditor
into the active authenticated Desktop key and PS/2 event path.

20 scoped, independently testable workstation capabilities (NOT 20
independently running code-writing agents):

1. Use the real cursor-aware LineEditor as the live InputLine backend.
2. Insert ordinary printable bytes at the authenticated caret position.
3. Backspace from the caret without corrupting the trailing text.
4. Forward Delete at the caret.
5. Navigate Left and Right through focused physical PS/2 events.
6. Navigate Home and End through focused physical PS/2 events.
7. Accept E0-prefixed extended navigation; reject arbitrary high bits.
8. Implement protected Ctrl+A/E Home/End bindings.
9. Implement protected Ctrl+B/F Left/Right bindings.
10. Implement Ctrl+W/Backspace word erasure and Ctrl+D forward delete.
11. Provide bounded full-range replacement and atomic multi-byte paste.
12. Provide validated per-session clipboard copy, cut, and paste.
13. Expose owner-checked, short-buffer-safe input/caret snapshots.
14. Retain up to eight successful commands in a bounded overwrite ring.
15. Deduplicate consecutive identical commands and exclude failed commands.
16. Navigate history with Up/Down and Ctrl+P/N while restoring the draft.
17. Scrub history and clipboard on exit, crash, restart and PID reuse.
18. Support Escape cancellation without performing durable I/O.
19. Complete unambiguous command prefixes using the real 30-verb registry.
20. Complete printable US set-1 punctuation/Shift mapping and refuse
    unknown Ctrl combinations rather than inserting unintended letters.

The implementation is allocation-free, bounded by MAX_LINE, and requires
an authenticated Desktop owner, PID-generation, current focused window
and live capability before private command/clipboard operations.
History recall and completion never execute a command without Enter.

R044 regression tests cover invalid input, selection rollback, capacity,
process isolation, capability expiry, real PS/2 scan mapping, caret and
history state machine, lifecycle scrubbing and safe command completion.
M1 CI explicitly invokes r041_, r042_ and r044_ test groups in addition
to prior M1/R040/R043, no_std, actual QEMU graphical Files and persistent
disk reboot. Security S0 and full native kernel boot remain mandatory.

Remaining M1 gates in issue #616 are NOT closed by host-side service tests:
real hardware IRQ-to-scheduler-to-ring3 input, independent ring3 compositor,
recovery and full normal-boot Desktop evidence remain outstanding. The
repository "Ten Modules One Build" workflow verifies code but does NOT
launch 20 autonomous code-writing agents.


### R045 — M1 G2/G6 read-only partition inventory and storage integrity preflight (2026-10-29)

This batch obeys the M1-first freeze in issue #616: no unrelated workstation
features, placeholder agent files, compatibility experiments or fake
percentage/gate-completion claims. Deliverables improve the installer/Files
preflight and failure evidence for safe persistent Desktop storage.

The following are 20 scoped engineering tasks, NOT 20 independently running
software-coding agents:

1. Immutable validated partition catalog with stable on-disk indices.
2. Single-pass defensive MBR discovery without implicit selection.
3. Single-pass whole-GPT discovery after primary/secondary CRC validation.
4. Keep GPT sparse partition indices rather than renumbering entries.
5. Reject GPT duplicate unique IDs across live non-overlapping entries.
6. Reject overlapping GPT/MBR partitions before exposing the catalog.
7. Validate unused slots before exposing later live records.
8. Fail atomically when catalog destination buffer is too small.
9. Return disk I/O failures without exposing unverified partial catalog data.
10. Distinguish a valid empty partition map from an invalid disk.
11. Recheck scheme/index/start/size against fresh partition metadata on mount.
12. Continue rejecting unformatted volumes; no mount-triggered autoformat.
13. Provide a hard read-only BlockDevice adapter for inspection APIs.
14. Combine the read-only adapter with bounded selected-sector windows.
15. Reject even a mistaken format request through a read-only window.
16. Report two independently checksum-verified COW metadata generations.
17. Report degraded single-copy volume health, never silently say healthy.
18. Reject divergent valid equal-generation split-brain metadata.
19. Detect corrupt latest data/metadata and report the valid fallback state.
20. Wire selected-partition health and geometry-locked mount into M1 Desktop.

Production modules: rust/kernel_core/src/{block,durablefs,hier_vfs,
m1_partition,m1_persistent_desktop}.rs. A read-only inspection cannot
write through the ReadOnlyDevice capability, even if a later caller
accidentally invokes write_sector. Read-only diagnostics use both COW heads
and actual file-payload checksums; no recovery repair is automatically
performed. Selected mounts still refuse a changed or unformatted partition.

New focused tests: 19 tests in block.rs, durablefs.rs and m1_partition.rs,
plus existing Desktop partition integration test extended with real
read-only health preflight and checked mount. Scenarios include sparse
MBR/GPT, duplicate and overlapping entries, secondary GPT corruption,
short outputs, simulated disk I/O errors, torn or invalid COW head,
corrupted current file payload, restored redundancy via a valid next
commit, selected partition remount and protected neighbor sectors.
The mandatory M1 workflow now explicitly runs the r045_ group alongside
earlier M1/R040-R044, freestanding no_std and real QEMU graphical
Files plus persistent scratch-disk reboot checks. S0 and full kernel boot
are still required on the exact integration head.

**Acceptance remains unchanged:** G2 and G6 (along with other eight
unchecked gates) need integrated physical/normal-boot and selected-QEMU
recovery evidence under issue #616. Service-level host tests and the
existing QEMU Files smoke alone do not close those high-level gates.


### R046 — M1 G6 crash-safe COW checkpoint recovery and authenticated storage repair (2026-10-29)

M1-first scope, following issue #616 rather than unrelated workstation feature
expansion. This change retains the existing R027 three-bank COW format, does
not introduce automatic formatting, and does not touch physical disks. It is
not a complete real-world power-loss/fsync guarantee: BlockDevice drivers
must meet their documented per-sector durable write/flush contract.

20 concrete G6 implementation/acceptance work items, NOT 20 separate running
coding agents:

1. Fail-closed validation of impossible nonadjacent metadata generations.
2. Reject CRC-valid divergent same-generation metadata during health probing.
3. Strict healthy-only mount for unattended normal-boot startup paths.
4. Explicit degraded volume reporting for recovery/diagnostic UI paths.
5. Inspect both live heads and checksum-verify all referenced file payloads.
6. Cross-check on-disk active metadata against the mounted in-memory head.
7. Refuse stale or externally modified mounted metadata before any repair.
8. Explicit metadata-only repair of exactly one invalid redundant head.
9. Verify the repaired metadata sector via immediate readback.
10. Revalidate both heads and all file checksums before repair acknowledgment.
11. Preserve every data bank and existing VFS object during redundancy repair.
12. Never rewrite an already healthy rollback generation (no-op repair).
13. Keep future three-bank COW writes valid after a successful repair.
14. Exercise power-loss failure at each of four COW data sectors and publish.
15. Exercise lying storage acknowledgments at each of those five stages.
16. Exercise torn metadata-only repair and preserve the previous committed file.
17. Exercise falsely acknowledged dropped metadata-only repair.
18. Expose audited health and explicit metadata repair on the actual HierVfs.
19. Expose authenticated, time-bound Desktop health and repair routes; reject
    stale PID, wrong owner, expired grants and untrusted Terminal commands.
20. Expose a read-only Terminal health command and strict checked-partition
    mount without autoformat; verify directory/files after reboot/remount.

Production modules changed: durablefs, hier_vfs, m1_partition,
m1_persistent_terminal and m1_persistent_desktop (Rust) plus the mandatory M1
workflow. Tests: twenty new tests prefixed m1_r046_ distributed across
those five real modules, including the real Desktop service, partition-bound
VFS tests and a fault-injected synthetic block device. The M1 CI workflow
explicitly runs the m1_r046_ subset; older M1/R040-R045, no_std, S0 and
QEMU graphical Files + scratch disk reboot remain mandatory.

Operational safety:
- Terminal "health" requires FILE_READ and reports only status, generation
  and number of checksum-verified metadata heads, not raw disk contents.
- Untrusted Terminal "repair" and "heal" strings are deliberately unknown.
- The only repair entrypoint is explicitly invoked through the scheduler-
  authenticated Desktop owner with live FILE_READ and FILE_WRITE grants.
- A successful metadata-only repair does not restore corrupted file contents
  or replace hardware/flush qualification. Never auto-heal on normal boot.
- mount_healthy refuses degraded storage; regular mount is retained for
  explicit controlled recovery. Read-only partition health inspection
  remains available for diagnosis.

Evidence status: OPEN until exact PR SHA passes all mandatory Rust, S0 and
QEMU checks. Do not mark G6 or any other eight outstanding M1 gates complete
based solely on host-side tests. Full independent ring-3 normal-boot,
physical device fault tests and fsync/write-barrier audit still pending.


### R047 — G5 authentic QEMU PS/2 keyboard and desktop input evidence (2026-10-29)

**M1 completion-first, not a 20-agent factory claim.** The prior graphical
Files QEMU test accepted only COM1 serial() input for all keyboard
shortcuts and therefore proved serial control, not guest PS/2 keyboard
delivery. This batch changes actual native kernel 8042 input and the
real QEMU test to require a hardware-emulated keyboard transition,
an authenticated Desktop action and visible framebuffer changes across
a persistent-disk reboot. The unchanged serial path remains a separate
control, never an acceptable substitute for the PS/2 gate.

20 scoped engineering and verification deliverables (NOT 20
independently coding workstations):
1. Use the native Set-1 PS/2 make-code path in the QEMU acceptance test.
2. Prove physical keyboard and COM1 serial inputs have different origins.
3. Increment physical evidence only after an accepted keyboard byte.
4. Emit a one-time in-kernel physical evidence marker, not a host echo.
5. Reject controller AUX mouse bytes from the keyboard decoder.
6. Preserve AUX bytes for the independent PS/2 mouse packet broker.
7. Reject 8042 controller parity and timeout errors.
8. Ignore all PS/2 break codes without submitting shortcuts.
9. Track left Ctrl separately from ordinary key content.
10. Track right Ctrl through E0-prefixed press/release codes.
11. Track left Alt independently of printable keys.
12. Track right Alt with E0 make/break transitions.
13. Block modified keys from issuing unmodified desktop/Terminal commands.
14. Consume the E1 Pause/Break sequence without leaving Ctrl stuck.
15. Accept genuine E0 up/down history navigation and reject releases.
16. Decode E0 keypad Enter and Slash without leaking extended codes.
17. Preserve independent left/right Shift and CapsLock semantics.
18. Prove physical launcher and Files/Terminal focus actions through live
    ring-3 serial acknowledgments and distinct VGA pixels.
19. Repeat physical keyboard verification after the same scratch disk
    is remounted on a second QEMU boot; keep real mouse and file guards.
20. Exercise physical-key shutdown and retain COM1, Files content, IPC
    refresh, ring-3 process reaping and neighbor-sector checks.

Production code: rust/kernel_core/src/input.rs (native x86 port-input
decoder and physical provenance) plus rust/kernel_core/src/ps2_mouse.rs
(AUX parity/timeout rejection, packet resync and guarded keyboard-vs-mouse
buffer ownership). Acceptance:
tools/test_r031_files_gui_qemu.py (HMP sendkey, no COM1 fallback,
actual framebuffer screendumps), plus mandatory
.github/workflows/m1-persistent-desktop.yml trigger and R047 host
regression invocation. The native QEMU test requires the real kernel
marker "R047 genuine PS2 keyboard byte accepted by kernel input" AFTER
each boot's physical sendkey, followed by an actual Desktop focus or
launcher acknowledgment and graphical pixel change. Host-only tests
cannot masquerade as guest QEMU evidence.

Acceptance limits: This improves G5 evidence but does NOT prove
IRQ-driven input, a full Terminal command typed by PS/2 into durable
HierVfs with GUI readback, or independent userspace compositor window
management. Native QEMU can use single-BSP 8042 polling (not necessarily
IRQ1); the current automatic Desktop boot remains feature-gated.
Do not close any of the eight outstanding high-level M1 gates from R047
alone. No physical host disk devices are opened or modified.

Execution: one integration branch, one squashed commit and one PR.
Mandatory CI runs seven pure decoder tests plus graphical-feature AUX
mouse regressions under the real GUI feature set. The QEMU test must
run its authenticated real-8042 sequence on both boots with the same
isolated disposable scratch disk. Any missing physical origin marker,
Desktop action or framebuffer change is a failure, never a serial fallback.

Actual independently executing coding workers launched through this
GitHub connector: 0. The 20 scoped items are testable work, not
20 live autonomous programming agents. Exact-head Rust/no_std, M1/QEMU,
S0 and full kernel boot pass remain required before merge.
