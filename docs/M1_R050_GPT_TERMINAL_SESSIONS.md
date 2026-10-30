# R050 — GPT-bound, fail-closed persistent Terminal sessions

**M1 priority:** G1, G2 and G6. This batch extends existing production Terminal and GPT/VFS APIs; it does **not** deliver all eight incomplete M1 milestone exit gates.

## Production behavior
- New `GptTerminalSession::open` requires current scheduler-derived PID/generation, active filesystem capability, valid session deadline, explicit fully checked GPT identity and healthy dual-checkpoint COW metadata.
- Each command independently rechecks the selected disk GUID, partition GUID, primary/secondary GPT metadata, geometry and filesystem health, then executes the existing durable VFS command parser under its own per-verb rights.
- A changed disk, corrupt GPT or volume mount error permanently retires the session. Reusing the original GUID later cannot revive it. A parser error leaves session state intact for user correction.
- Authenticated `status`, bounded lifetime attenuation and owner-only `close` provide a safe lifecycle, including PID generation isolation.
- The session does not format, repair or access arbitrary physical storage. It is an API requiring trusted process identity from the kernel; it is **not** proof of ring-3 syscall integration or normal desktop boot.

## Acceptance checks
The added `m1_r050_` Rust regression group checks real COW file writes, rename, read-after-remount, independent reopen, owner/expiry and readonly denials, invalid input/short buffer atomicity, stale GPT and disk GUID changes, mirrored GPT corruption, degraded COW refusal, write interruption and post-failure recovery, neighbor-sector isolation and permanent revocation. These tests are included in the existing mandatory `m1_` test filter of `m1-persistent-desktop.yml`.

**Required before merge:** exact-PR-SHA offline Rust host, freestanding no_std, actual disposable-disk QEMU Files/Terminal reboot acceptance, S0 security, kernel-boot QEMU, independent review and current `main` checks. No physical disk writes. Do not weaken or skip CI.

## Agent/workstation accounting
Twenty agent workstations were requested. The available GitHub connector cannot start or coordinate twenty independent autonomous coding sessions; **0 coding agents were launched in this execution**. This is one coordinator-produced substantive M1 G1/G2/G6 slice, not a claim that twenty features or the broad M1 milestone have been completed. The twenty pending priority tasks remain in `docs/M1_RELIABILITY_BATCH.md`.
