# M1 G1–G8 automated acceptance policy

This is the active milestone-acceptance procedure for issue #616 positions
6–13. It extends PR #827's test-based G1 rule to G2–G8. It changes the
decision method, not the behavior required for each gate. Older packets,
checklists and registry notes are historical where they prescribe a separate
gate reviewer, recording session, manual checkbox or sign-off. The five
foundation gates, all substantive requirements, GitHub branch protection
and required pull-request checks are unchanged. No physical disk is written
for this test-based policy.

## Decision and reporting

The [read-only GitHub Actions evidence report](https://github.com/pramod07info/indianos/actions/workflows/agent-independent-reviewer.yml)
checks trusted exact-main evidence, including required job and step results
and the same-job retained QEMU artifact. Missing, failed, skipped, stale
or expired mandatory evidence cannot establish a new pass. Pull-request
results are candidate evidence only. G2–G5 additionally require their
named gate-specific receipt checks; the existing QEMU assertions still run. A failure in an added gate receipt or the standalone G3 repeat prevents that gate from counting, without negating a valid result for the other gate. The existing shared regression remains mandatory for both.
The report must be read for individual outcomes: a successful evaluation
job alone is not a gate pass.

A verified milestone is attributed to its accepted SHA and evidence links.
A later main revision lacking its own complete verification is unverified
at that revision; it does not erase the earlier result. Passing at an earlier
revision is not proof that a new revision works. No separate reviewer or
manually set acceptance checkbox is required.

## Gate evidence and blockers

| Gate | Use existing checks | Additional proof needed for acceptance |
| --- | --- | --- |
| G1 | Normal Terminal input, authenticated durable writes, selected-volume reboot and graphical readback; shared regression matrix | Existing exact-main rule from PR #827. |
| G2 | MBR parser/mount tests, mirrored GPT negative cases, selected-volume bounds and unchanged media, ATA write failure, cold remount | Require the separately named G2 receipt step, green shared checks and retained QEMU evidence. Native MBR desktop boot and physical hardware are not extra G2 conditions. |
| G3 | Production four-service desktop, per-role negatives, missing/invalid media, live restart, bounded retirement and shutdown exhaustion | Require two independent unready-compositor boots with the unchanged deadlines, plus the named G3 receipt step. A failure in either boot prevents G3 acceptance, and existing diagnostics are retained. The second boot is a separate CI assessment, not a retry-to-green. This qualifies the case but does not assert a root cause for the historical timing failure. |
| G4 | Existing full-frame QEMU and PID4-only recovery in PR #816 | Require the G4 retained-evidence step and exact-revision M2 non-regression on top of the common workflows. The same QEMU job must retain four distinct process roots, PID1/PID4 authority-denial receipts, guarded app surfaces, complete frames, fresh client-preserving recovery and all three strict fallback cases. |
| G5 | Existing PS/2 keyboard, pointer, focus/capture and physical-input desktop oracles | Require the separate G5 live PS/2 verification step: normal keyboard/mouse IRQ ingress, physical focus and capture, recovery from an emulated i8042 AUX overflow and keyboard overrun, quarantine of stale input, and fresh visible Files and Terminal effects. UART injection and host tests alone cannot count. No physical machine is required for this gate. |
| G6 | Existing COW publication cuts, identity checks, live leases and file-lock regressions | Block until scoped concurrent-writer, per-user and flush-order qualification is complete and the required physical cache-loss evidence is available. Ordinary reboot or a simulated publication cut cannot substitute for required physical proof. |
| G7 | Existing native descriptor and read-lease tests plus process-specific denials | Block until the full native independent-process create/read/edit/rename and denial matrix proves the expected bytes survive same-disk reboot. Broker-only or host-only paths cannot count. |
| G8 | Existing emulator driver, shutdown, recovery and negative security coverage | Block until exact-revision selected physical device and firmware qualification, safe shutdown/restart and remaining negative test exits are demonstrated. Emulator drivers do not prove physical devices. |

G6–G8 already use this policy but are deliberately reported as BLOCKED
until their dedicated receipts are implemented: do not replace their missing evidence with a generic
green workflow or a sign-off. Integrate each completed gate's automated
receipt into the existing report, without creating a second ledger or
weakening the original [gate scope](https://github.com/pramod07info/indianos/issues/616).

Prior verified baseline under PR #827: G1 accepted at
`42dc173421fd5e1d2ab0fbd1c2f3b0f77c262d44` (6/13, 46.15%),
[normal desktop QEMU](https://github.com/pramod07info/indianos/actions/runs/46151671023/job/140999439531).
Either G2 or G3 adds 1 gate (7/13, 53.85%); both add 2 (8/13, 61.54%)
only after the active policy is merged and required evidence passes.

Verified before enabling G4: G1–G3 accepted at
`87b9e9b1b594c7c5162b3d9e7d599546098aef77` (8/13, 61.54%).
A new verified G4 acceptance makes M1 9/13 (69.23%).

Verified G5 acceptance with G1–G4 retained makes M1 10/13 (76.92%).
