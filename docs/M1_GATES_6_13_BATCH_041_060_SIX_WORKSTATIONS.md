# IndianOS M1 third batch — master IDs 041–060 targeted
Date 2026-11-17. Parent 11d301fbda155d407558f91cfac13afdeefff278. Open draft PR #784.
These are **20 new task IDs selected**, not 20 end-to-end accepted requirements.

| Workstation | Gate | New master IDs | Actual source implementation | Still unimplemented / unaccepted |
| --- | --- | --- | --- | --- |
| W1 session | 8 | 024,025,026,029 | Verified launch+settings cohort atomic staging and exact owner snapshot | Full users/groups/clock/notifications and graphical login |
| W2 service lifecycle | 8 | 027,028,030 | Transactional exact-generation shutdown group and pending-service rejection | Real daemons, reboot/suspend, backup/recovery UI |
| W3 ring3 isolation | 9 | 031–035 | Atomic SG pointer preflight, guard/overflow/alias denials | Live page tables, copy_to_user, fault recovery and compositor |
| W4 CPU | 13 | 077,078,080 | Protected-PID memory-pressure victim selection and exact epoch queue eviction | Actual SIMD context, OOM reclaim and preemptive multicore scheduler |
| W5 IRQ/DMA/driver | 13 | 085–087 | Revalidated signed-stage resource owner/BDF/IOMMU numeric proposal | Active IRQ/MSI, DMA driver isolation and physical resource binding |
| W6 driver fault/hotplug | 13 | 088–089 | Exact-owner software virtqueue quiescence on changed device BDF | Actual hardware hotplug and IRQ/DMA shutdown |

Master IDs: 024, 025, 026, 027, 028, 029, 030, 031, 032, 033, 034, 035, 077, 078, 080, 085, 086, 087, 088, 089.

Six CI verification jobs run previous m1_gate20_wN and new m1_batch03_wN filters
plus freestanding compilation at exact PR head. No six autonomous code-authoring
agents were launched. All original master checkboxes stay OPEN until full QEMU,
physical qualification, independent review and protected merge evidence.
