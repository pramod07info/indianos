# IndianOS Orbit — screen compatibility and UI coverage

**October 27, 2026 | R002 initial requirement and implementation ledger**

## Definitions

"Works on all screens" means **two distinct things**: every primary OS
screen/application has functioning controls, and supported screens adapt to
different dimensions/input devices. Pixel rendering alone does not count as
working apps or hardware support.

## Resolution and input acceptance

| Profile | Typical viewport(s) | R002 status | Release gate |
|---|---|---|---|
| Compact QEMU | 640x480, 800x600 | Host-tested geometry; real multi-mode boot pending | QEMU boots, two windows draw with no clipping, focus, keyboard & pointer |
| Laptop | 1024x768, 1280x800, 1366x768 | Layout classifier and adaptive geometry; 1280x800 real QEMU acceptance available in R001 | QEMU screenshots and input actions at **each** chosen resolution |
| Full HD / high res | 1920x1080, 2560x1440 | Not supported by present 1600-wide VGA renderer | Checked framebuffers/driver mode enumeration, physical or QEMU screenshot, scaling & performance |
| 4K/5K, high DPI | 3840x2160+ | Not implemented | Proper text scale, fonts, cursor hit targets, multi-monitor coverage, memory/performance budget |
| Ultrawide | 3440x1440, 5120x1440 | Not implemented | Usable workspaces, taskbar and window layouts with no dead or clipped controls |
| Tablet landscape | 1024x768 + touch | Responsive **geometry only** | Touchscreen HID driver, gestures, virtual keyboard, auto-rotation |
| Phone portrait | 390x844, 412x915 | Pure **UI layout model**, not a boot target | ARM64 boot, trusted framebuffer, multi-touch driver, radio/audio/power and phone usability |
| Multi-monitor / external USB-C | Mixed per-monitor dimensions and DPI | Not implemented | Hotplug, independent EDID/mode, per-monitor color/scaling, window relocation |
| Accessibility | Zoom, 200% text scale, Hindi and English | Not implemented | Font fallback & shaping, screen readers, contrast and full keyboard navigation |

The experimental x86 VGA backend currently rejects viewports below 640x480
and above 1600x1600. **R002 does not remove that guard.** Layout unit
tests on mobile/wider model sizes are not hardware support claims.

## Functional application-screen ledger

A screen is completed only when it opens from the real launcher, performs its
primary task, handles error/permission states, and survives restart/reboot
when persistence is part of its task.

| Screen | Current status | Acceptance requirement |
|---|---|---|
| Desktop/workspaces | Orbit R001 visual preview | Select workspace, launch authorized apps, persistent desktop on default boot |
| Launcher/search | Label/layout preview | Search installed approved apps and launch/revoke safely |
| Files | Experimental QEMU GUI | Create/edit/move/copy/delete/list arbitrary authorized files and reopen after reboot |
| Terminal | Experimental QEMU client | Keyboard shell with real durable VFS and process ownership |
| Settings | Placeholder | Display, language, themes, networking and power apply and persist |
| Login/lock | Not implemented | Account auth, session lock, idle timeout, recovery |
| App Store | Not implemented | Signed package approval, install/update/remove & rollback |
| Browser/web | Not implemented | Sandboxed secure browser and network integration |
| Text editor | Not implemented | Open/edit/save/restore across reboot |
| Task monitor | Not implemented | Show and manage actual processes with scoped authority |
| Notifications/quick settings | Not implemented | Actionable events without granting unsolicited rights |
| Orbit Assistant | Preview only | Explicit review/approve/revoke; truthful local/cloud privacy indicators |
| Session Ribbon | Preview only | Verified opt-in restore of metadata, not fake process memory snapshots |
| Mobile dialer/messages/camera | Not implemented | Verified radio stack, sensor/camera drivers, privacy and emergency-call requirements |

## Delivery policy

- Share exact-SHA **real QEMU screenshots** for the supported resolution,
  not an AI mockup or a screenshot generated only from model state.
- Host tests must cover narrow and wide layout bounds, while integration
  tests exercise actual window/client commands and pointers.
- Do not claim a screen/hardware class "working" until its matching row has
  a reproducible acceptance artifact.
- Correctly distinguish desktop PC, tablet and phone: phone compatibility
  is a **different platform port**, not only responsive visuals.
