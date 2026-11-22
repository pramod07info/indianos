# Checklist positions 6 and 7: Terminal and partition acceptance

These are issue #616 **G1** and **G2**, not the later G6 persistent-VFS and G7
cross-process API gates. The implementation and local runtime matrix below
have positive exact-source **and merged-main** QEMU/host evidence from PR #790,
verified 22 Nov 2026 (details below). The formal gate acceptance still
requires independent review and sign-off. Do not count these gates as accepted
or add duplicate kernel code solely because the milestone checkboxes are open.

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
