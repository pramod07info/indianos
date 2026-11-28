# Checklist positions 6 and 7: Terminal and partition acceptance

> **Current rule:** See [G1–G8 automated acceptance](M1_G1_G8_AUTOMATED_ACCEPTANCE.md). The gate behavior and historical receipts below are preserved; older gate-level review and sign-off instructions are superseded. GitHub merge rules are unchanged.

These are issue #616 **G1** and **G2**, not the later G6 persistent-VFS and G7
cross-process API gates. The implementation and local runtime matrix below
have positive exact-source **and merged-main** QEMU/host evidence from PR #790,
verified 22 Nov 2026 (details below). G1 and G2 use the current test-based policy linked above. Do not add duplicate kernel
code solely because acceptance evidence is pending.

## G1 test-based acceptance (PR #827 historical baseline)

This section activates the G1 portion of the proposal in PR #810 and
supersedes older G1/G2/G3 joint review language for **G1 only**. At the time of PR #827, G2, G3
and all other gates kept their then-existing rules. GitHub branch protection
and required merge settings remain unchanged, including any review they
require for the policy PR itself.

For G1, the [existing Actions evidence job](https://github.com/pramod07info/indianos/actions/workflows/agent-independent-reviewer.yml)
makes the acceptance decision on main. It reuses the existing exact-SHA
checks: combined host/freestanding and real normal-desktop/runtime QEMU,
Durable Files, kernel boot, Security S0, Roadmap SHA and Duplicate
Prevention; the same-revision desktop artifact must be retained from the
successful job. The real keyboard, authenticated permissions, visible
Files results and two cold boots with exact remount readback remain
mandatory. Missing, failed, skipped, stale or expired required evidence
cannot produce PASS. A PR-head or pre-policy report cannot accept G1.

The Actions summary records the revision, required results, run IDs and
artifact ID. A G1 ACCEPTED result on main running this policy is the
acceptance record and raises the foundation count from 5/13 to 6/13
(46.15%). No separate independent source approval, explicit human
sign-off or manual checklist edit is a condition. Any issue or checklist
display is informational for G1. Results are revision-bound: later
changes do not inherit verification from this revision.

## Position 6 / G1: live authenticated durable Terminal

| Required behavior | Implementation and observed evidence |
|---|---|
| Real ring3 keyboard-to-filesystem path | Normal Desktop PID3 Terminal sends canonical authenticated IPC to PID1; selected syscall32/33 operations reach actual ATA-backed HierVfs |
| Read/write/mkdir/mv | Physical input executes named touch/put/mv/rm/mkdir/cat; exact directory state is checked after every structural mutation |
| Correct permissions/session | Actual Files pointer/private-path/write-escalation, Terminal direct-FS, compositor input/FS and window-authority probes must precede fresh readiness; host suites cover incarnation/revocation cases |
| Real GUI damage | Structural operations must change visible Files pixels and render the exact expected directory glyphs; shortened fields must erase trailing pixels |
| Reboot/remount/readback | Two cold QEMU/OVMF boots reuse the same selected disk; exact `VIOLET` file bytes and GUI glyphs return after remount, with the saved filename and directory visible in Files |
| No stale success masquerading as data | The named payload is deliberately different from `SAVED`; stale/missing/wrong glyphs, trailing white pixels and late authority evidence fail host negative controls |

The independent glyph oracle checks the full qualified text field, including
blank padding, rather than trusting a UART success label or any screenshot
change. It uses the fixed normal Desktop geometry and bitmap font; arbitrary
themes/layouts are outside this bounded acceptance contract. UART input is a
negative control and cannot operate the physical-input desktop.

## Position 7 / G2: safe selected partitions

| Required behavior | Implementation and observed evidence |
|---|---|
| Defensive MBR/GPT discovery | Read-only full-catalog validation; MBR bounds/overlap tests and GPT primary/secondary header/array CRC, identity and geometry tests |
| Protective MBR bounds | Exact remaining capacity on small disks; 32-bit saturation only when required; zero/short/oversized entries reject without writes |
| Overlap/duplicate-ID rejection | Real QEMU boots use semantically invalid but correctly checksummed, mirrored GPT images; rejection happens before user services start |
| Bounded explicit selection | GUID/bounds pin and complete receipt revalidation; BlockWindow guards and full parent-disk comparisons prevent neighboring-sector writes |
| Actual remount | The same selected GPT volume survives real emulator reboot and serves the exact GUI/readback checks above |
| Write failure | A blkdebug backend permits genuine ATA identification/read/admission, then fails writes with EIO; Terminal renders `IO ERROR`, the retired mount refuses reads and sync, and the entire disk stays unchanged |
| Corruption/recovery/no autoformat | Invalid GPT/COW/namespace and changed-identity cases enter read-only recovery without service startup, formatting or media changes |

The firmware-facing semantic-fault disks use the same write-failing backend so
OVMF cannot normalize malformed metadata before kernel inspection. A valid-media
positive control with that backend must still boot the desktop; a missing drive
cannot masquerade as layout or write-failure acceptance. Fixture unit tests prove
both GPT mirrors/CRCs and COW payload health remain valid before introducing each
named layout fault, and repaired positive controls are accepted.

MBR discovery and mount paths are covered by host integration; the normal
Desktop's trusted boot selection currently targets GPT. Physical driver/media
qualification belongs to the later G8 gate and is not an extra G1/G2 prerequisite.
Tests never write a physical disk. This does not claim G6 power-loss/ACL/locking,
G7 direct multi-process API completion or overall M1 release readiness.

## Verification and acceptance boundary

Local combined suites passed: 3,378 Desktop kernel, 3,435 legacy runtime, 3,432 M2,
142 Python, five fixture, two volume-tool and one real-filesystem CLI test.
Freestanding compilation, optimized native Desktop ISO build, whitespace checks
and local S0 source-contract audit passed. Feature-suite test counts overlap.

The complete matrix is twenty actual VM boots: the prior sixteen plus three
partition semantic rejections and one real ATA write-failure case. Current logs
are `gates6-7-*.log` under `/workspace/indianos-gates-validation/`; fresh serial,
PPM and rejected-image evidence is retained under the normal Desktop artifact
directory. CI runs the same harness against its exact checked-out PR head and
uploads the artifacts. A failed/skipped required check prevents gate acceptance.

The above historical PR #765 candidate is superseded by exact-head PR #790
and its verified merged-main SHA below. The code paths and the required
QEMU cases now pass the verified merged-main checks. The checklist remains
open solely for independent code review, explicit acceptance decision and
the gate ledger's corresponding sign-off evidence.

## G1/G2 consolidated exact-head candidate, 22 Nov 2026
Existing G1 Terminal and G2 GPT/MBR functions have strong host/QEMU evidence.
Focused PR #790 additionally protects complete receipt identity across
commands and GPT sessions. Three more CRC-valid GPT identity/mirror rejection
fixtures now run in the real QEMU partition-negative matrix (25 configured
boots, not yet accepted on this revision). Host fixture tests check checksum
correctness, unaffected filesystem data and zero writes on discovery failure.

G1/G2 remain open until exact-head full normal-desktop QEMU, kernel boot,
security, combined reliability, independent review, and merged-main
confirmation all pass. Do not infer review or production physical-device
qualification from source changes.


## G1/G2 measured exact-SHA evidence — audited 22 Nov 2026

**Evidence is not a new feature implementation.** PR [#790](https://github.com/pramod07info/indianos/pull/790)
merged as `f928b87aeffcefbc5772f054aa14a5fbda0c8436`.
The merged code was inspected for pre-existing implementations in PRs
#618, #632, #639, #652, #654, #765 and #778. Optional overlapping
MBR multi-command session PR #792 was closed **without merge**; don't
restart it without a new reproducible gate deficiency.

| Acceptance track | Exact candidate SHA `f1194992` | Merged-main SHA `f928b87a` |
|---|---|---|
| Combined reliability, host/no_std and actual normal-desktop QEMU | [44976597643](https://github.com/pramod07info/indianos/actions/runs/44976597643) — success | [44984574278](https://github.com/pramod07info/indianos/actions/runs/44984574278) — success |
| Real graphical Files / durable block-VFS | [44976597653](https://github.com/pramod07info/indianos/actions/runs/44976597653) — success | [44984574330](https://github.com/pramod07info/indianos/actions/runs/44984574330) — success |
| Full kernel boot | [44976597615](https://github.com/pramod07info/indianos/actions/runs/44976597615) — success | [44984574248](https://github.com/pramod07info/indianos/actions/runs/44984574248) — success |
| Security S0 | [44976597597](https://github.com/pramod07info/indianos/actions/runs/44976597597) — success | [44984574259](https://github.com/pramod07info/indianos/actions/runs/44984574259) — success |
| Checkout SHA integrity | [44976597659](https://github.com/pramod07info/indianos/actions/runs/44976597659) — success | [44984574296](https://github.com/pramod07info/indianos/actions/runs/44984574296) — success |

On the **merged-main** combined reliability run, all three jobs
(`normal-desktop-qemu`, `runtime-qemu`, `combined-regression`) completed
successfully. The actual QEMU log reported both `Normal desktop boot 1/2`
and `2/2` PASS, the committed file surviving an abrupt guest loss, and
read-only media recovery for overlapping/duplicate-ID, nil/changed GUID,
backup-GPT mismatch, protective overflow, corrupt GPT/COW, namespace
corruption and selected-media invalidation. The injected **actual ATA
write failure** case passed with refusal of sync, unchanged disk and
a retired mount. QEMU evidence also includes physical PS/2 input,
four ring-3 services and exact durable file readback. The workflow's
25-case boot matrix completed successfully; unlike the earlier
"configured 25" note, these are confirmed **run results**.

**Retained artifacts:** combined-reliability run
[44984574278](https://github.com/pramod07info/indianos/actions/runs/44984574278)
stores `normal-desktop-acceptance-f928b87aeffcefbc5772f054aa14a5fbda0c8436`
(artifact ID `14739029463`, about 7.4 MB) and
`m1-runtime-acceptance-f928b87aeffcefbc5772f054aa14a5fbda0c8436`
(artifact ID `14739680150`, about 90 KB). Their current 14-day retention
expires on **6 Dec 2026**. Archive any required independent-review
screenshots, logs and byte-identity evidence before expiration.

### Remaining acceptance only — no new production code authorized yet

- [x] Exact-PR-head M1, security, kernel and QEMU tests green.
- [x] Post-merge `main` revision exact-sha kernel/security/desktop/QEMU tests green.
- [x] Log-confirmed two cold boots, partition-corruption/failed-write negative
      controls, and uploaded evidence artifacts.
- [ ] **Independent security/correctness code review** of the existing
      storage receipt/GPT mirror/ATA failure, identity-generation, and
      false-ACK semantics. PR #790's submitted GitHub review list was empty
      when audited on 22 Nov 2026. Automated green runs are not approvals.
- [ ] Explicit gate-acceptance sign-off citing this merged SHA and review
      evidence, followed by marking checklist positions 6–7 in
      `docs/DELIVERY_MILESTONES.md` accepted in a separately reviewed
      change. Do not claim 7/13 until that has occurred.

**Scope exclusions:** MBR native desktop boot beyond the existing verified
host mount contract, physical drive identity attestation/physical-media
testing, G6 crash-atomicity, and G7 general public file ABI are different
requirements. They must not delay G1/G2 acceptance by generating duplicate
Terminal or partition APIs.

## Automated G1–G3 evidence triage

The existing `agent-independent-reviewer.yml` runs a separate read-only evidence
job on its existing schedule (twice hourly), PR events, kernel workflow completion,
and manual dispatch. It examines current main and relevant same-repository PRs,
including drafts, checks the latest exact-SHA workflow/job/required-step results,
and verifies that the retained desktop artifact was created by the successful
job, not a previous failed attempt. Missing, skipped, stale or expired evidence
blocks readiness. The Actions job summary lists each required workflow and run ID.

This job is an automated evidence reviewer, **not** an independent code approval.
It does not submit approvals, label PRs, merge, or modify gate counts. The existing
bounded legacy approval policy is unchanged. A separate correctness/security
review and explicit gate sign-off remain mandatory; the G3 intermittent recovery
timeout also requires investigation. Workflow policy changes themselves require
independent review before trusted-base activation.

## G1–G3 independent source review and explicit sign-off — 24 Nov 2026

**Review target (merged production baseline):** `5b6ace8a6e5d91c217744e5ad0b8ca7f02ace1d7` (PR #806).
This packet is a request for a separate reviewer to inspect production source,
not an acceptance decision. G1, G2 and G3 remain open and M1 remains 5/13.
The same approval must not be inferred from a merge, a green workflow, an
implementation author's self-review, a bot report, or this packet.

### Revision-bound evidence

PR #806 candidate `2e06fcc5a7f371d41b068c01ac58f86ac304ce24` passed the
[combined reliability run 45382847991](https://github.com/pramod07info/indianos/actions/runs/45382847991)
(all three jobs). The [normal desktop log](https://github.com/pramod07info/indianos/actions/runs/45382847991/job/138406054114)
reports 4,218 kernel tests, zero failures, all four premature-exit cases,
missing/invalid Files and Terminal, unready compositor, bounded restart,
read-only GPT/ATA error recovery and two cold boots. The three new live-map
tests passed. [Retained candidate artifact 14914758401](https://github.com/pramod07info/indianos/actions/runs/45382847991/artifacts/14914758401)
contains the acceptance evidence. Candidate evidence is not a substitute for
merged-main verification.

Latest main CI at packet creation (snapshot only; reviewer must refresh the
live run, job, step and artifact results before signing):

| Required workflow | Exact-main run | Observed status |
| --- | --- | --- |
| Combined reliability | [45395190978](https://github.com/pramod07info/indianos/actions/runs/45395190978) | in_progress |
| Durable Files/VFS | [45395191443](https://github.com/pramod07info/indianos/actions/runs/45395191443) | success |
| Kernel boot | [45395191063](https://github.com/pramod07info/indianos/actions/runs/45395191063) | in_progress |
| Security S0 | [45395191360](https://github.com/pramod07info/indianos/actions/runs/45395191360) | success |
| Roadmap SHA | [45395191179](https://github.com/pramod07info/indianos/actions/runs/45395191179) | success |
| Duplicate prevention | [45395190913](https://github.com/pramod07info/indianos/actions/runs/45395190913) | success |

The failed [run 45377901348](https://github.com/pramod07info/indianos/actions/runs/45377901348)
and [artifact 14913822725](https://github.com/pramod07info/indianos/actions/runs/45377901348/artifacts/14913822725)
must also be considered. The subsequent successful QEMU run is evidence that
the failure did not recur after the frame-query performance correction, not
proof that every intermittent timeout cause was eliminated. Do not reduce the
45-second deadline or ignore a failed recovery case.

### Independent review assignment (all required per gate)

Reviewers must be authorized repository collaborators with independent
judgment, not the author/implementer of the reviewed changes. A single
qualified independent person may cover several gates if competent in those
areas; no fictitious reviewer or workstation is required. The existing
Actions evidence reviewer assists, but does not grant source approval.

| Gate | Review scope at the exact target | Required observations |
| --- | --- | --- |
| G1 | [rust/kernel_core/src/m1_persistent_terminal.rs](https://github.com/pramod07info/indianos/blob/5b6ace8a6e5d91c217744e5ad0b8ca7f02ace1d7/rust/kernel_core/src/m1_persistent_terminal.rs), [rust/kernel_core/src/m1_persistent_desktop.rs](https://github.com/pramod07info/indianos/blob/5b6ace8a6e5d91c217744e5ad0b8ca7f02ace1d7/rust/kernel_core/src/m1_persistent_desktop.rs), [rust/kernel_core/src/hier_vfs.rs](https://github.com/pramod07info/indianos/blob/5b6ace8a6e5d91c217744e5ad0b8ca7f02ace1d7/rust/kernel_core/src/hier_vfs.rs), [rust/kernel_core/src/fs_handles.rs](https://github.com/pramod07info/indianos/blob/5b6ace8a6e5d91c217744e5ad0b8ca7f02ace1d7/rust/kernel_core/src/fs_handles.rs), [userland/rust/src/main.rs](https://github.com/pramod07info/indianos/blob/5b6ace8a6e5d91c217744e5ad0b8ca7f02ace1d7/userland/rust/src/main.rs) | Authenticated sender/incarnation, selected-volume identity, mutation and namespace permissions, no false success/ACK, durable reboot readback, visible Files state and negative authority controls. |
| G2 | [rust/kernel_core/src/m1_partition.rs](https://github.com/pramod07info/indianos/blob/5b6ace8a6e5d91c217744e5ad0b8ca7f02ace1d7/rust/kernel_core/src/m1_partition.rs), [rust/kernel_core/src/block.rs](https://github.com/pramod07info/indianos/blob/5b6ace8a6e5d91c217744e5ad0b8ca7f02ace1d7/rust/kernel_core/src/block.rs), [rust/kernel_core/src/durablefs.rs](https://github.com/pramod07info/indianos/blob/5b6ace8a6e5d91c217744e5ad0b8ca7f02ace1d7/rust/kernel_core/src/durablefs.rs), [rust/kernel_core/src/ata_pio.rs](https://github.com/pramod07info/indianos/blob/5b6ace8a6e5d91c217744e5ad0b8ca7f02ace1d7/rust/kernel_core/src/ata_pio.rs) | MBR/GPT bounds and mirrors, CRC/identity/overlap rejection, complete receipt revalidation, no neighbor writes, fail-closed mount retirement, ATA EIO and unchanged media, no autoformat. |
| G3 | [rust/kernel_core/src/kernel_main.rs](https://github.com/pramod07info/indianos/blob/5b6ace8a6e5d91c217744e5ad0b8ca7f02ace1d7/rust/kernel_core/src/kernel_main.rs), [rust/kernel_core/src/boot.rs](https://github.com/pramod07info/indianos/blob/5b6ace8a6e5d91c217744e5ad0b8ca7f02ace1d7/rust/kernel_core/src/boot.rs), [rust/kernel_core/src/user_runtime.rs](https://github.com/pramod07info/indianos/blob/5b6ace8a6e5d91c217744e5ad0b8ca7f02ace1d7/rust/kernel_core/src/user_runtime.rs), [rust/kernel_core/src/user_image.rs](https://github.com/pramod07info/indianos/blob/5b6ace8a6e5d91c217744e5ad0b8ca7f02ace1d7/rust/kernel_core/src/user_image.rs), [userland/rust/src/desktop_startup.rs](https://github.com/pramod07info/indianos/blob/5b6ace8a6e5d91c217744e5ad0b8ca7f02ace1d7/userland/rust/src/desktop_startup.rs), [userland/rust/src/main.rs](https://github.com/pramod07info/indianos/blob/5b6ace8a6e5d91c217744e5ad0b8ca7f02ace1d7/userland/rust/src/main.rs), [tools/test_normal_desktop_qemu.py](https://github.com/pramod07info/indianos/blob/5b6ace8a6e5d91c217744e5ad0b8ca7f02ace1d7/tools/test_normal_desktop_qemu.py) | Normal desktop entry and four actual ring-3 services; ELF W^X, authenticated readiness, complete stale authority and frame retirement, one restart, shutdown dispatch exhaustion, durable shutdown, individual negative boots and live-map validation without weakening safety. |

Inspect actual diffs and source as well as serial, monitor and screenshot evidence.
Record concrete findings and their disposition. If review discovers a defect,
open a focused fix with a regression; do not add duplicate production features.
If a reviewer cannot verify a requirement, report BLOCKED rather than approve.

### Review and acceptance protocol

1. The coordinator identifies an actual independent authorized reviewer for each
   scope and requests a GitHub review of this packet. No review is recorded
   merely by assigning a role. Source links above refer to the fixed production
   revision, not the documentation-only packet.
2. The reviewer checks the latest exact-main required CI and retained artifacts,
   and performs the source/security review against the pinned baseline. If
   production source or required test/policy changes, refresh the packet and
   repeat impacted verification. An approval of unrelated PR content is not
   source approval for these gates.
3. Record a submitted GitHub approval on the current review-packet head, with
   an explicit gate decision in the review body. Use the following format,
   replacing every placeholder with actual findings and receipts. Approve only
   independently inspected gates; unreviewed gates stay pending.

   ```text
   Reviewed production SHA: 5b6ace8a6e5d91c217744e5ad0b8ca7f02ace1d7
   Gate: G1 | G2 | G3 (one entry per gate)
   Decision: ACCEPT | BLOCKED
   Source/security findings: <paths, symbols, findings and disposition>
   Exact-main CI: <required run/job IDs and verified conclusions>
   Retained evidence: <artifact IDs and checked serial/screenshot cases>
   Remaining mandatory blockers: <none for ACCEPT, otherwise specific blockers>
   ```

4. After an eligible independent approval and explicit ACCEPT decision exist,
   the coordinator reconciles the canonical registry, the corresponding
   acceptance document, master milestone positions 6–8 and tracker #616 in
   a separately reviewed acceptance change. Record reviewer identity, GitHub
   review URL/ID, reviewed SHA, exact-main CI/artifact receipts and decision
   for each gate. Verify that acceptance-change CI is green and production
   scope is unchanged before publishing the count. Do not let the review
   packet itself advance a checkbox. G1 = 6/13, both G1/G2 = 7/13, all three
   = 8/13 only after their respective formal acceptance changes.

No independent review has been recorded for PR #806 at packet creation.
The repository owner and GitHub Actions bot appear in recent commit history;
a distinct eligible reviewer has not yet been confirmed. Reviewer assignment,
live merged-main validation and acceptance decisions remain open. This section
does not weaken branch protection or existing review requirements.
