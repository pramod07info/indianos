# IndianOS Orbit — evidence-first UI implementation

**Status:** R001 in opt-in QEMU preview on `feature/orbit-ui-r001-qemu-preview`.
**Baseline:** `1c6dd5451266567aed76a2407c005fdfd81f8726`.
**Important:** The visual concept from October 27, 2026 is a design target, **not evidence of functioning AI, search, snapshot restore, security or third-party apps**.

## Differentiator

A goal-oriented OS where workspaces, commands, apps, files and approved AI actions
are integrated into a clear **review-before-apply** interface. Features
similar to parts of this exist in Windows/macOS/Linux; claims of global uniqueness
must be backed by real research and user studies.

## R001 — a real visible improvement, not just isolated backend models

- Allocation-free framebuffer `orbit_ui` painter integrated directly with the actual
  R029 QEMU VGA compositor and the ring-3 surface command bridge.
- Navy/teal/gold Orbit visual language; goal workspace rail (Build, Create, Learn,
  Personal), top command/search **placeholder**, Files and Terminal window chrome,
  assistant status **placeholder**, session ribbon **placeholder**.
- Workspace selection via `b/c/l/p` in the opt-in QEMU R029 interactive shell.
  This selection changes the rail and workspace label; it does not change the
  underlying process, grant ownership or launch apps.
- Files and Terminal remain actual supervised graphical windows; background
  shapes and titles do not change process grants or issue new filesystem rights.
- `make test-orbit-ui-qemu` validates on QEMU/OVMF with live user-mode drawing,
  protected chrome checks and a saved framebuffer artifact.
- `orbit-ui-preview` is **off by default**; normal production boot remains unchanged.

## R002 — functioning desktop shell

- Real authenticated app launcher (not a decorative navigation rail).
- Surface clipping, focus, drag/resize, input events and window protocol
  separated into trusted ring-3 compositor service.
- Functional Files, Terminal and Settings, safe restart and real keyboard/mouse
  interaction in QEMU.
- Acceptance: screenshot **and** reproducible user task, normal boot mode
  and cold-reboot persistence tests.

## R003 — dependable session ribbon

- Session metadata with explicit storage boundaries, opt-in checkpoints and
  crash-safe restoration. Never claim arbitrary process memory is restored.
- User-visible restore preview; show which files/windows/processes will re-open,
  and block replay of revoked capabilities or sensitive tokens.
- Check signed manifests, disk restore after reboot, corruption fallback,
  no silent overwrite and refusal of forged per-user history.

## R004 — Orbit Assistant with permissions

- Explicit local/remote model selection, no hidden network calls, source
  disclosure and scope-limited workflow proposals.
- Before applying any change, show exact files, commands, permissions, risks
  and rollback plan; require human approval and block hostile prompt injection.
- Do not claim safety because a model recommended an action.

## R005 — visual completeness and portability

- Unicode shaping for Hindi and English, accessibility/keyboard navigation,
  zoom/HiDPI, high-contrast and theme system, rich icons and polished assets.
- Render on real supported GPU/display drivers, performance measurements,
  screen-reader testing, physical machine qualification and secure updates.

## Acceptance / evidence

Every visual release requires (a) exact commit SHA, (b) actual QEMU
screendump artifacts, (c) pixel checks with visible before/after
difference, (d) functional keyboard/mouse tests, (e) Rust/no_std CI
and (f) no regression in default desktop boot. No mocked UI screenshot
may be labeled as a running IndianOS build.
