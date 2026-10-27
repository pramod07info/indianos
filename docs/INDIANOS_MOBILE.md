# IndianOS Mobile — proposed ARM64 edition

**Status: feasibility/engineering roadmap only. No phone kernel boot has
been implemented or verified.** Desktop x86-64 Orbit remains the current
priority and secure integration foundation.

## Product goal

Share one **IndianOS Orbit** experience across PC, tablet and phone while
tailoring each device's input, density, battery life and system services.
A mobile home screen should be touch-first, with bottom navigation, gesture
support, compact Assistant approval flows, large targets, lock screen and
offline operation. Docking to a monitor could optionally switch to Orbit's
desktop presentation after hardware qualification.

## Technical architecture

- **Platform-neutral services:** Rust file abstractions, permission policy,
  app model, session metadata, UI layout description and approved assistant.
- **Kernel/platform port:** separate AArch64 entry, exception/interrupt and
  timer handling; page-table/MMU, SMP and power states; UEFI or device-specific
  boot chain; native ARM64 user ABI. Existing x86-64 assembly must not be
  presumed portable.
- **Hardware abstraction:** selected open development board or phone with
  documented boot process; hardware description (device tree/ACPI); DRM/
  framebuffer/GPU, display panel, capacitive multitouch, audio, Wi-Fi,
  Bluetooth, modem/SIM/eSIM, camera/ISP, sensors, battery/charging and
  suspend/resume. Proprietary vendor firmware may be required.
- **Security:** verified/rollback-aware boot and updates, key storage and
  encryption, sandboxed app lifecycle, mandatory per-app camera/microphone/
  location permissions, lock screen and protected emergency-call behavior.
- **App compatibility:** IndianOS native ARM64 apps first. Android app
  compatibility would require significant runtime/framework integration
  and testing; AOSP is built around Linux kernel, ART and HAL. Do not claim
  Android APKs, iOS apps or Windows apps execute on IndianOS Mobile by default.
- **Release constraints:** phone-specific radio/regulatory testing, OTA rollback,
  thermal and battery qualification, accessibility, security review, and
  device-by-device support matrix before flashing real user-owned phones.

## Staged mobile gates

| Gate | Objective | Evidence |
|---|---|---|
| MOB-0 | Phone portrait/responsive Orbit layout planner | no_std unit tests at 360/390/412 widths |
| MOB-1 | AArch64 boot on QEMU virt | Verified boot log, exception path and memory model |
| MOB-2 | Touch-capable graphical compositor, virtual keyboard | QEMU or documented emulated touch screenshots/interactions |
| MOB-3 | Network, storage, signed native ARM64 app launch | Persistent disk, scoped network and app acceptance |
| MOB-4 | One documented phone/device board bring-up | Display, touch, Wi-Fi and power measured on real hardware |
| MOB-5 | Phone functions and security assurance | Modem/SIM, microphone/speaker, camera, OTA and emergency functions passed |

**Recommended development order:** finish usable PC Orbit M1 first;
in parallel keep the core UI logic architecture-neutral and start **MOB-0**
tests. Avoid building an Android fork if the goal remains IndianOS's
independent kernel.

References:
- AOSP architecture: https://source.android.com/docs/core/architecture
- ARM64 architecture: https://docs.kernel.org/arch/arm64/
