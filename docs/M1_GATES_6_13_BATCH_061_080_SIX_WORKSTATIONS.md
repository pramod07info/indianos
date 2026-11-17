# M1 fourth 20-task kernel/desktop foundation batch

Date: 2026-11-17. Parent: `f56ea86cca199240498de6f673d378cf4e3b9c50`. Target: existing draft PR #784.
**Selection is not acceptance**: the 20 distinct task IDs below track real
source work but do not certify full GUI/hardware milestones.

| Verification workstation | Gate | IDs | Integrated kernel/service functionality | Still outstanding |
|---|---|---|---|---|
| W1 Files/Terminal/editor and partition | 6/7 | 001–004 | Generation-pinned authenticated VFS copy/replace preserving persistent ACLs; negative owner and remount tests; two-pass read-only MBR/GPT batch selection | Actual interactive ring3 file-manager, editor UI, terminal event path, multi-process device integration |
| W2 disk/ATA | 7 | 005–007 | Atomic multi-sector metadata read that leaves output unchanged on driver failure; LBA28 bounds enforced in real ATA selector | Hardware ATA qualification, GPT/MBR exhaustive device coverage, full removable media lifecycle |
| W3 private surfaces | 9 | 036–038 | Exact-owner transactional surface damage delivery and screen-coordinate validation with rollback | Actual ring3 compositor, privately mapped framebuffers and hardware display |
| W4 window operations | 9 | 039–041 | Batch z-order hit test and authenticated window activation | Unicode font/shaping and full userspace window manager |
| W5 themes/input/UI | 9 | 042–046 | Atomic cross-session theme changes and checked UTF8 scalar offsets for future font layout | UI themes/icons, acceleration, DPI, clipboard/drag-drop, compositor recovery, complete Unicode |
| W6 PS/2 ownership | 10 | 051–052 | Stale-focus resistant Set-1 scan batch admitted through existing exact-generation keyboard queue | Real PS/2 controller IRQ tests, USB/xHCI and power/resume validation |

**New distinct master IDs:** 001, 002, 003, 004, 005, 006, 007, 036, 037, 038, 039, 040, 041, 042, 043, 044, 045, 046, 051, 052.
**All 90 master checkbox acceptance items stay OPEN**, even though implementation
slices are present. Workstations W1–W6 are independent GitHub Actions verification
jobs, not separate autonomous code-writing agents. Existing `m1_gate20`,
`m1_batch03` and new `m1_batch04` test families plus freestanding build must
pass at the exact PR head. Physical secure boot, power-loss, hardware IRQ/DMA,
actual normal desktop QEMU, and independent review are required separately.
