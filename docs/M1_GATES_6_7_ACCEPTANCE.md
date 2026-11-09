# Checklist positions 6 and 7: Terminal and partition acceptance

These are issue616 **G1** and **G2**, not the later G6 persistent-VFS and G7
cross-process API gates. The implementation and local runtime matrix below are
complete. Final gate acceptance requires successful checks on the exact new
PR765 head and repository review; the accepted main baseline does not advance
merely because a candidate commit was pushed.

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

See PR765's exact head/checks for final CI status. Until those results and the
repository review requirements are satisfied, the source checklist remains open
despite completed implementation and local exit evidence.
