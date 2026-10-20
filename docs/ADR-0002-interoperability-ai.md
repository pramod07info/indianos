# ADR 0002: platform interoperability and AI-first installation

- Date: 2026-10-20
- Status: accepted as long-term design direction; implementation pending
- Scope: application compatibility, assistant planning, hardware portability and GitHub workflow

## Decision

1. Preserve IndianOS as an **independent kernel**.
2. Treat universal cross-platform support as an **aspiration**, not a guarantee. Implement native apps and measured compatibility provider routes (web, POSIX/Linux, Win32, guests, Android, authorized remote Mac), with visible per-app results.
3. Keep CPU architecture portability separate from cross-OS app compatibility. x86-64 first, design for ARM64 after kernel maturity.
4. AI can translate natural language into a **declarative app request**; deterministic, audited, signed privileged services alone can install software, subject to the user's explicit authorization.
5. Default GitHub development policy for this repository is **direct commits to main**, with tests and clear commit history. Create a pull request only when repository owner explicitly requests it. This does not remove security reviews or release-quality gates.

## Consequences

- Requires major compatibility engineering and new kernel facilities; third-party Linux, Windows and macOS applications do not run natively today.
- No proprietary binaries, macOS installation on unsupported hardware, circumvention of DRM/signing or unsupported license claims.
- AI install UX must display source provenance, application permissions, execution route, compatibility level and rollback plans.
- Continue shipping small, tested milestones without inventing progress.
- Direct-to-main changes should be bounded, reversible and tested where feasible. Critical software must still meet independent verification before public release.

## Related designs

- COMPATIBILITY.md
- AI_APP_ASSISTANT.md
- PERFORMANCE_GOALS.md
