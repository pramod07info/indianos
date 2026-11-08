# Checklist positions 8–10: boot, isolation and device input

These are issue #616 gates G3, G4 and G5, not its later G8 hardware gate.
All three remain **IN PROGRESS** until required checks, exact-revision QEMU
artifacts and review satisfy their exit criteria. Source changes alone do not
accept a gate.

| Position | Required behavior | Implementation and remaining acceptance |
| --- | --- | --- |
| 8 / G3 | Automatic normal boot to isolated PID1, Files, Terminal and compositor; recovery for missing/bad services or media | Trusted Desktop edition selection, native role/feature enforcement, strict service startup join, kernel readiness, bounded cooperative startup and one verified restart are implemented. Require final-head normal boot, invalid-service/media and withheld-readiness QEMU results. |
| 9 / G4 | Independent ring-3 compositor/input broker, guarded per-app surfaces, event-driven lifecycle and negative isolation | PID4 owns window policy; PID1 brokers physical input through authenticated IPC. Private command payloads have source/cloned-root guard mappings. Input authority follows readiness/shutdown; stale drag capture and client grants are revoked. Require final-head mapping, forbidden-syscall and crash/restart evidence. General shared application pixel buffers remain unimplemented. |
| 10 / G5 | Real keyboard/mouse actions, focus/capture, Terminal/Files changes and screenshots | Physical Set-1 and AUX ingress stay separate from UART. Overrun/loss cancels unsafe command fragments; pointer packets preserve button edges. Acceptance now requires actual PS/2 focus, drag, release, minimize/restore and exact file glyphs. Require final-head device-input QEMU artifacts; these are emulated devices, not physical-machine qualification. |

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
