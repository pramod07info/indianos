# Checklist positions 8–13: boot, isolation, input and persistence

These are issue #616 gates G3 through G8. The existing filename is retained
for published links. All six remain **IN PROGRESS** until required checks, exact-revision QEMU
artifacts and review satisfy their exit criteria. Source changes alone do not
accept a gate.

| Position | Required behavior | Implementation and remaining acceptance |
| --- | --- | --- |
| 8 / G3 | Automatic normal boot to isolated PID1, Files, Terminal and compositor; recovery for missing/bad services or media | Trusted Desktop edition selection, native role/feature enforcement, strict service startup join, kernel readiness, bounded cooperative startup and one verified restart are implemented. Require final-head normal boot, invalid-service/media and withheld-readiness QEMU results. |
| 9 / G4 | Independent ring-3 compositor/input broker, guarded per-app surfaces, event-driven lifecycle and negative isolation | PID4 owns window policy; PID1 brokers physical input through authenticated IPC. Private command payloads have source/cloned-root guard mappings. Input authority follows readiness/shutdown; stale drag capture and client grants are revoked. Require final-head mapping, forbidden-syscall and crash/restart evidence. General shared application pixel buffers remain unimplemented. |
| 10 / G5 | Real keyboard/mouse actions, focus/capture, Terminal/Files changes and screenshots | Physical Set-1 and AUX ingress stay separate from UART. Overrun/loss cancels unsafe command fragments; pointer packets preserve button edges. Acceptance now requires actual PS/2 focus, drag, release, minimize/restore and exact file glyphs. Require final-head device-input QEMU artifacts; these are emulated devices, not physical-machine qualification. |
| 11 / G6 | Power-loss-safe writes, flush ordering, directories, locking and per-user isolation | Checkpoint publication now validates all referenced payloads before/after metadata writes and after the final durable flush. Uncertain mounts retire. Corruption/transport/flush fault tests include explicit remount outcomes. General per-user ACLs, concurrent locking and physical power-loss qualification remain open; a bounded fault test does not close this gate. |
| 12 / G7 | Independent processes use authenticated file syscalls and retain files across reboot | Existing PID1/PID2/PID3 file flows are supplemented with Enter-bound, one-request Terminal broker transactions and an actual unsolicited mutation denial. Authenticated cached seek/close no longer depend on device availability; closing a reader releases its writer exclusion without media I/O. Require exact-head syscall/reboot evidence. Stable public ABI versioning and crash-persistent request identifiers remain open. |
| 13 / G8 | Qualified QEMU/physical drivers, safe shutdown/restart, crash recovery and negative security tests | ATA IDENTIFY now verifies a declared integrity checksum before admitting capacity. Existing bounded I/O retirement and verified service recovery remain. Physical-device qualification, ACPI power control and wider driver support are not established by these changes. |

The normal acceptance workflow runs kernel host/freestanding checks, startup
and window-policy host tests, harness negative controls, native image builds
and the actual 22-boot QEMU matrix. Added pointer actions run within the two
healthy boots. Exact readback after reboot, unchanged neighboring storage,
malformed media and service recovery remain mandatory; no existing criterion
is removed to make an input test pass.

At `615c3ccc6cebd068bdb78296700fbe32ff5abe58`, GitHub M1 run
[42511425101](https://github.com/pramod07info/indianos/actions/runs/42511425101)
passed combined/runtime jobs but failed the new window-policy host regression.
The fixture sent ASCII `P1` although the established presentation wire format
is the byte `P` followed by binary window ID 1. The fixture is corrected;
production parsing still rejects malformed controls. Normal Desktop QEMU was
skipped at that revision and supplies no acceptance evidence for this batch.

At `f641f55ec7a8121b75d1dec9cf662456891ddc3e`, run
[42512047977](https://github.com/pramod07info/indianos/actions/runs/42512047977)
passed combined/runtime jobs and normal Desktop host/build checks. Its first
healthy Desktop boot passed; boot two failed strict framebuffer identity after
pointer focus. Retained screenshots show the first pixel sampled between the
background fill and top-bar repaint. The harness now requires three identical
real frames before comparison and retains every settlement sample. It still
requires full framebuffer equality, with bounded failure on unstable output.
This correction was pushed separately as `b0462d3`; final-head QEMU evidence
remains pending.

ATA integrity validation is conditional on the optional word-255 A5 signature.
When declared, the sum of all 512 IDENTIFY bytes must be zero before sector
geometry can be accepted. Legacy devices without the declaration retain their
existing admission rules. Tests reject corruption in every response word and
preserve legacy compatibility. This is error detection, not device identity,
authentication or proof that physical storage is safe to install onto.

This batch was authored without executing local builds, tests or QEMU, per the
requested workflow. Git whitespace checks and static review are not runtime
proof. New GitHub results must reference the integrated source revision.

Remaining cross-cutting limits include cooperative single-BSP execution,
polling input in the normal profile, CPU-hog containment, shared pixel-buffer
support, broader USB/HID/GPU drivers and physical-device qualification. None
is established by a screenshot or a successful compilation. See
[boot selection](M1_NATIVE_DESKTOP_BOOT.md),
[lifecycle](DESKTOP_LIFECYCLE_BOUNDARY.md),
[guarded surfaces](M1_GUARDED_COMMAND_SURFACES.md),
[PS/2 recovery](M1_PS2_INPUT_RECOVERY.md),
[physical delivery](M1_PHYSICAL_INPUT_DELIVERY.md), and
[actual QEMU criteria](M1_NORMAL_DESKTOP_ACCEPTANCE.md).
Storage details: [checkpoint admission](M1_CHECKPOINT_PAYLOAD_ADMISSION.md) and
[broker transactions](M1_TERMINAL_BROKER_TRANSACTIONS.md), plus
[descriptor release](SELECTED_DESCRIPTOR_RELEASE.md).
