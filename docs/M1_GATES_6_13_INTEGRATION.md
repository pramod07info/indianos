# M1 desktop integration: gates 6–13

Base main: `2e9dfaebbf34d964981d4914980a41c677d9e3e2`. This is one
coordinated source batch, not a claim that every M1 release gate is complete.
Implementation was collected before the combined build; subsequent rebuilds
address concrete failures found by that validation.

## Actual implementation

- A `normal-desktop` production profile, without experimental feature aliases,
  packages and preflights four real ELF services. An explicit INDVOL01 boot
  selection pins GPT identity and bounds; missing services/configuration or
  unaccepted media enter read-only recovery. No guest provisioning occurs.
- PID1 brokers physical input and selected-volume file operations. PID4 owns
  window focus, stacking, movement, minimization, launcher and pointer policy.
  Syscall37 transfers checked window state to a clipped kernel raster backend;
  old focus/frame/control syscalls cannot bypass the compositor in this profile.
- All normal file/path/list/sync calls use the admitted GPT receipt and current
  process/namespace authority. The old raw scratch-volume fallback is refused.
- The graphical Terminal supports bounded named touch/put/cat/rm/mkdir/mv under
  `/shared`, in addition to existing note commands. The broker independently
  validates the canonical IPC request. Files tolerates changed directories and
  a missing/empty sample note. Successful mutations refresh actual Files pixels.
- Physical ingress excludes UART, rejects poisoned/lost input and clears stale
  session state. PS/2 motion uses the actual signed nine-bit wire format.
- Failed sync verification retires the mount; uncertain ATA operations retire
  their session. Shutdown requires a successful selected-volume sync first.
- The Desktop edition has its own validated `make desktop-image` entry and
  default graphical boot menu. A read-only host tool pins an explicitly chosen,
  healthy existing GPT image using the actual kernel parser and filesystem.
- A service fault permits one whole-session restart only after verified resource
  retirement and healthy exact-volume revalidation. Process generations, tokens
  and quotas persist. A second fault enters recovery; clean shutdown never retries.
- Selected path mutations refuse foreign open descriptors before ATA access.
  Shared readers retain their authority until authenticated close/revocation.
  These cooperative volume-wide leases are not general SMP or POSIX locks.
- Selected descriptor mutations now enforce the same read leases across write,
  truncate, edit, positioned/conditional write, create, unlink and rename APIs.
  Authentication precedes conflict admission, which precedes device inspection;
  mounted dispatch independently checks the conflict. Read/seek/close and fsync
  remain available, and unselected legacy-volume behavior is preserved.

The desktop uses four distinct trap stacks. QEMU exposed the old two-stack
feature selection and then a double fault during the deeper GPT/COW save path.
The desktop now provisions four 128KiB usable trap stacks and a 16KiB DF stack,
each with a real unmapped lower guard page. Effective page permissions and guard
absence are checked in the source and cloned roots before process publication.
SMP, large-stack-jump protection and intentional overflow containment remain
outside this bounded verification.

## Gate ledger

| Position / gate | Source and current evidence | State / remaining exit work |
|---|---|---|
| 6 / G1 | Authenticated native named CRUD/mkdir/mv; exact Files damage/listing and distinct VIOLET GUI readback across two real boots | Local exits verified; exact-head CI/review required for acceptance |
| 7 / G2 | Exact protective-MBR bounds, both GPT mirrors, selected-window guards, actual semantic rejection/write-failure and reboot evidence | Local exits verified; exact-head CI/review required for acceptance; physical qualification is G8 |
| 8 / G3 | Four-service native Desktop edition entry, preflight, one bounded verified restart and recovery | IN PROGRESS: installation and final-head qualification |
| 9 / G4 | Independent PID4 window manager, PID1 input broker, authenticated syscall37, isolated app command streams and fault-loop acceptance | IN PROGRESS: broader stress and guarded surface/service qualification |
| 10 / G5 | Actual emulated PS/2 keys/mouse, focus/actions and screenshots; UART negative control | IN PROGRESS: final acceptance matrix and IRQ/hardware qualification |
| 11 / G6 | Selected persistent COW, sync, path/descriptor read leases, error retirement, unit faults and SIGKILL/reboot readback | IN PROGRESS: general locking, per-user ACLs, torn-write guest and physical power/flush qualification |
| 12 / G7 | Real independent services use authenticated live path/descriptor calls; same-disk reboot readback | IN PROGRESS: broader process permissions, stress and final-head matrix |
| 13 / G8 | Service/media rejection, guarded trap stacks, bounded persistent-fault recovery and successful interactive restart after a real transient ring3 page fault | IN PROGRESS: overflow/fault stress, ACPI power/reboot and selected physical drivers |

No high-level gate is newly marked PASS by this draft. Existing accepted
foundations remain 5/13. Full hardware trust, SMP, ACLs, physical qualification
and complete overflow qualification are not implemented by this batch.

## Validation at draft creation

- Normal desktop host suite: 3,350 passed, including the trap-stack regression.
- Existing combined experimental host suite: 3,408 passed.
- Terminal parser/codec: 13 passed; actual userland window policy/wire: six passed.
- Python full suite: 127 passed; the focused normal desktop harness suite
  passed all eleven controls, including canonical named readback and fault ELF.
- Native optimized ELF/ISO and all four userland services built successfully.
- Real QEMU completed both physical-input selected-GPT boots, including named
  save/readback, Files refresh, sync and all four process reaps.
- All six service/configuration/media rejection scenarios passed without writes.
  The first single-header GPT fixture was repaired by OVMF before kernel entry;
  a separate real boot verified only that header changed. Both-header corruption
  now explicitly tests kernel rejection; no corruption check was disabled.
- A valid compositor ELF with UD2 at its executable entry produced an actual
  PID4 fault/reap. The kernel retired its peers and returned to read-only recovery
  with unchanged selected media. The failed service now stops dispatch promptly;
  rollback authorization is checked after peer cleanup rather than before it.
- All 14 GitHub workflows passed the first draft revision. Final follow-up
  changes require fresh exact-head CI; a previous revision's success is not reused.

Actual artifacts and failure logs are retained under
`build/normal-desktop-artifacts/` and `/workspace/indianos-gates-validation/`.
Do not merge this draft until its final source passes all applicable checks and
repository review requirements. No hardware completion percentage is inferred.

## Completed local follow-up batch

Three genuine independent coding sessions contributed production changes in
isolated worktrees; all three completed. No additional push is implied by this
local result. Fresh combined verification, after source integration:

- Native desktop host suite: **3,356 passed**, zero failed.
- Combined legacy runtime host suite: **3,413 passed**, zero failed.
- M2 developer-alpha host suite: **3,410 passed**, zero failed.
- Python full suite: **130 passed**, zero failed.
- Volume-selection tool: two unit tests and one real-filesystem CLI integration
  passed; filesystem-handle focused suite: 141 passed; restart permission: one
  passed. These overlap with the broader suites and are not added to their totals.
- The dedicated Desktop edition built an optimized bootable ISO. Actual
  QEMU/OVMF passed two clean persistent graphical boots, two abrupt-loss/reboot
  boots, all six rejection cases and the two-fault/one-restart recovery scenario.
  Fresh artifacts: `build/normal-desktop-artifacts/run-w0e_fowh`.
- Abrupt loss uses QEMU SIGKILL **after** commit acknowledgement. It proves
  unclean guest reboot readback, not a torn write or physical power-loss safety.
- Local S0 source-contract audit passed. This is not independent release review.

GitHub M2 run `42082374211` on the previously pushed `f2cfbed` revision failed
while installing QEMU prerequisites: the Ubuntu Azure mirror stalled downloading
`qemu-system-data`, and the bounded installation exited 124. Earlier host and
freestanding steps passed; its guest tests were skipped, not passed. The failed
job was retried once without changing source or weakening checks. New local
source has not inherited that earlier revision's CI status. Release acceptance,
merge and all eight outstanding high-level gates remain open.

## Next gate implementation: descriptor conflict protection

Seven new regressions cover all mutating descriptor actions, unchanged outputs,
budgets and cursors on denial, protected readers of unrelated files, close and
retirement release, successful write retry, mounted-dispatch bypass rejection,
fsync with readers and unchanged legacy preflight behavior. The torn-write test
still injects an actual partial media write after lease release and verifies full
quarantine; it additionally proves the lease prevents premature device access.

Independent review found that the preflight also serves unselected legacy
volumes. The initial change was too broad; selected-only admission and the new
legacy regression resolve that compatibility defect. After this fix the actual
combined suites passed **3,363 normal-desktop**, **3,420 legacy-runtime** and
**3,417 M2 host** tests, all with zero failures. The dedicated Desktop ISO rebuilt
successfully and the local S0 source-contract check passed. Logs are retained as
`next-gate-*-reviewed.log` and `next-gate-security-contract.log` under the validation
directory. These host results do not close the complete G6 locking/ACL gate.

On 2026-11-07 GitHub reported all 14 workflows successful on the previously
published `f2cfbed` revision, including the retried M2 job. That result applies
only to the old published source. The newer local implementation remains
unpublished and needs its own exact-SHA CI before any merge.

The complete integrated Python suite passed **135 tests**. The final QEMU run
passed **12 VM boots**: two clean persistent graphical boots, two abrupt-loss
save/readback boots, one successfully recovered transient compositor-fault boot,
six rejection cases and one bounded deterministic repeated-fault boot. Artifacts:
`build/normal-desktop-artifacts/run-s86m7f9w`; full log:
`/workspace/indianos-gates-validation/next-gate-qemu.log`. The transient test uses
a local emulator debugger to verify PID4/ring3/code identity, redirect only user
RIP to an unmapped address and observe a real page fault. Fresh service execution,
physical launcher/editor input, durable named save/readback, sync and all four
clean reaps are required after restart. No production fault-injection hook exists.

The first focused debugger probe timed out because the physical trigger was sent
while the VM was paused. Sending it after confirmed resume fixed the sequencing.
The next probe completed the real interactive recovery but its final checker
miscounted the replacement compositor's legitimate clean reap as a duplicate
fault reap. Separate fault/shutdown phases and negative controls resolved that
harness defect; the final full run passed without dropping either reap check.
Both diagnostic failures remain in the validation directory. Independent review
found no remaining blocker in the final descriptor and acceptance diffs.

The user subsequently authorized publication, and `0a1789c` was pushed to PR765.
All fourteen workflows passed on that exact published revision. Those results do
not qualify subsequent source. No new high-level M1 gate, hardware qualification
or completion percentage is claimed from the earlier results.

## Same-PR follow-up: guarded stacks and namespace admission

The cloud workspace was reset; the coordinator restored the exact published
branch and a fresh Rust1.99.1/QEMU toolchain. Prior local artifact directories
were not restored, so this batch reran compilation and applicable suites rather
than claiming the older local logs still existed. Main remained `2e9dfaeb`.

- Actual lower guard pages protect the normal desktop's four trap stacks and DF
  IST stack. Source-owned split tables are installed before the PMM rollback
  baseline; huge-page PAT/address attributes and neighbors are retained.
- Effective RW/U/S/NX permissions are walked across every paging level. Review
  identified that the old leaf-only query could miss readonly ancestors; a
  separate stack-specific query and regressions fixed that before publication.
- Strict healthy mounts now reject inconsistent hierarchical namespaces despite
  valid GPT/COW checksums. They validate parent chains, aliases, markers and links
  before returning authority; no implicit repair or formatting occurs.
- Four disposable semantic-corruption images are generated by the actual COW
  writer. QEMU must reject each before user startup with unchanged media.

Actual final local checks: **3,376 desktop**, **3,433 legacy-runtime**, **3,430 M2**,
**138 Python** tests, plus **4 fixture**, **2 volume-tool** and **1 CLI integration**
tests passed. The feature suites overlap. Freestanding compilation, optimized
Desktop ISO build, whitespace check and local S0 source-contract audit passed.
The initial test compile exposed an incorrect borrowed test-device type; its
ownership was corrected without changing production block interfaces.

The real QEMU suite passed **16 boots**, including all previous twelve cases and
four valid-checksum namespace rejections. Its debugger read all five guards as
unreadable and usable stack endpoints as readable in PID4's cloned root and the
reclaimed recovery kernel, with different actual CR3 values. This is real mapping
evidence, not an intentional kernel-stack-overflow or hardware qualification.
Artifacts: `build/normal-desktop-artifacts/run-a72yw5fo`; current logs:
`/workspace/indianos-gates-validation/guard-namespace-*.log`.

See `M1_KERNEL_STACK_GUARDS.md` and `M1_NAMESPACE_ADMISSION.md` for implementation,
ownership and unsupported cases. All eight high-level gates remain IN PROGRESS.
This follow-up is authorized for the same PR, not a merge; fresh exact-head CI is
still required after push.
