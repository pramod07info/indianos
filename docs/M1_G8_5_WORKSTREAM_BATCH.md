# M1 G8 — Driver, Recovery and Shutdown Safety (2026-11-12)

Source base: `b6c719e3751a3ff26af8687c1d0ee14216084aa4`. This single-commit batch divides development into
five independently scoped engineering workstreams. **No five autonomous
coding agents were launched**; the work was integrated in one session.

| Workstream | Production implementation | Fault / regression test | Evidence status |
| --- | --- | --- | --- |
| 1 — ACPI firmware admission | Reject nonzero ACPI 2+ reserved RSDP bytes and unaligned RSDT/XSDT pointers before use | `g8_firmware_rsdp_v2_rejects_reserved_bytes_and_bad_root_alignment` | Source only; CI pending |
| 2 — ATA driver admission | Reject impossible LBA28 IDENTIFY sector counts instead of silently clamping geometry | `identify_lba28_bounds` and `identify_requires_lba_and_capacity` | Source only; CI pending |
| 3 — Crash classification | Do not treat external hardware IRQ vectors (32+) as recoverable process faults or consume restart budgets | `g8_irq_or_invalid_owner_cannot_consume_crash_retry_budget` | Source only; CI pending |
| 4 — Supervisor shutdown | Refuse all-services shutdown when an unprepared replacement reservation is still pending | `g8_shutdown_cannot_complete_with_unprepared_restart_reservation` | Source only; CI pending |
| 5 — PS/2 transport handoff | Fail closed if AUX firmware draining never reaches controller idle within the bounded 64-byte budget; do not steal keyboard data | `g8_aux_drain_never_steals_keyboard_or_trusts_unbounded_firmware` | Source only; CI pending |

## Acceptance still open

These changes are **bounded software defenses**, not proof of complete M1 G8.
Require actual exact-commit QEMU desktop boots; selected-partition/file
readback, safe power-off/restart and recovery; firmware/ATA/PS2 rejection
cases against real emulator I/O; guarded stack-overflow/fault stress; independent
security review; selected real-hardware driver and ACPI qualification.
A host-only callback test cannot qualify a real PS/2 or firmware controller.
No physical disk must be written by a test. All new execution evidence must
be associated with the exact PR head; skipped tests are not passes.

M1 issue #616 and docs/DELIVERY_MILESTONES.md remain the release contract.
Do not mark G8 complete based on this single commit or PR.
