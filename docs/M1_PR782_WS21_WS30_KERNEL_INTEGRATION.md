# M1 PR #782: 10 additional kernel-first implementation tracks (WS21–WS30)

Parent PR commit: `a3e6e0feb7904f4c776da4438051276699cd1f35`. This is ONE follow-on integration commit on the
same draft PR, not 10 independently executed agents and not hardware completion.

| Workstream | Real source change | Acceptance focus |
|---|---|---|
| WS21 Driver lifecycle | Checked MMIO extent and quarantine retirement only after privileged hardware quiescence | Wrong generation, active grant and unsafe DMA state reject |
| WS22 AHCI | Strict structural DMA geometry plus external IOMMU mapping-ownership callback | Invalid LBA before callbacks, unowned segment denies |
| WS23 NVMe | Generation+namespace+LBA+DMA-bound command completion | Mismatched completion cannot consume in-flight request |
| WS24 VirtIO | Optional nonaliasing checked split-ring DMA descriptor chain | Overlap denies; nonoverlap accepts |
| WS25 ACPI | Bounded transactional numeric XSDT/RSDT entry decoding | Duplicate, misaligned, insufficient output fail closed |
| WS26 DHCP | Reject malformed noncontiguous IPv4 netmask and non-padding bytes after end | Valid options with zero pad survive |
| WS27 Crash diagnostics | Bounded owner+generation sequence paging | Incremental cursor isolates other process crashes |
| WS28 Platform qualification | Reject contradictory passed/failed/skipped hardware evidence | Exact same SHA and platform still required |
| WS29 Files GUI | Exact process-generation authenticated metadata updates | Wrong owner and path aliases deny with no mutation |
| WS30 Release audit | Require main-head SHA match to candidate before declaring reviewed release | Green PR SHA never certifies an unmerged main |

## Acceptance required for this PR head

- [ ] All GitHub Actions workflows on exact PR head succeed, no disabled or skipped mandatory checks
- [ ] Freestanding Rust/no_std build, host tests, kernel boot, S0, durable VFS/desktop, R114 and R126 QEMU evidence
- [ ] Independent review, protected merge, and post-merge main SHA validation

**Important scope:** DMA ownership callbacks and quiescence booleans must be
issued by privileged driver code; these software APIs do not create an IOMMU,
hardware interrupt remapping, real TLB shootdown, power-loss-safe filesystem,
authenticated firmware trust chain or completed M1 gates.
