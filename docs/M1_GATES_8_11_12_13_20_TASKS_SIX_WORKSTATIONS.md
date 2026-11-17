# M1: 20 targeted master tasks across six verification workstations

Date: 2026-11-17. Scope: Gates 8, 11, 12 and 13. Existing PR #784.
**The 20 master task IDs below are targeted, not accepted.** Six CI workstations are real independent GitHub Actions matrix jobs for validation; they are **not** six autonomous code-writing agents.

| Workstation | Gate | Selected master IDs | Implemented slice and outstanding acceptance |
|---|---|---|---|
| W1 boot/initramfs | 8 | 008, 011, 014, 015 | Full Limine map preflight before exposing count; reject empty PID1 image. Still need full UEFI boot, recovery and corrupt-module QEMU evidence |
| W2 PID1/runtime | 8 | 016, 017, 020 | Dependency-ordered start verifies exact active PID+generation and excludes pending/stale services. Still need scheduler/process-reap integration and fault tests |
| W3 VFS integrity | 11 | 055, 056, 057, 058, 061 | Validate hierarchical namespace before redundancy repair, health report or sync. Still need physical powercut/corrupt-media, timestamps and journal acceptance |
| W4 file locks and mount | 11, 12 | 059, 067 | Reject unsafe selected-volume directory renames before disk probing; block window boundary regressions. Still need concurrent multiwriter ring3 storage |
| W5 capabilities/syscalls | 12 | 062, 063 | Guard-page user-copy rejection despite overlapping segment; transactional capability delegation to multiple recipients. Still need complete live ABI/IPC |
| W6 kernel/hardware | 13 | 070, 071, 083, 090 | Prevent unsafe TSS stack mapping output; exact PCI BDF diagnostic snapshot is all-or-none and read-only. Still need real PMM reclaim, PCI IRQ/DMA/IOMMU and physical qualification |

**Exact master IDs (20):** 008, 011, 014, 015, 016, 017, 020, 055, 056, 057, 058, 059, 061, 062, 063, 067, 070, 071, 083, 090.

## Merge and gate acceptance

1. Each of six matrix workstation tests and freestanding feature checks must pass at the final PR head.
2. Existing M1 combined reliability, normal QEMU desktop, persistent VFS/remount, kernel boot and Security Gate CI must pass at the same head.
3. Independent review must verify code; do not merge a draft PR on tests alone.
4. Preserve all 90 master ledger checkboxes unchecked until full acceptance evidence is attached and verified, especially hardware/physical tests.

Non-claims: six software engineers did not independently code; 20 accepted master tasks, a real SMP scheduler, physical DMA/IOMMU, and a complete bootable production desktop are not demonstrated by this batch.
