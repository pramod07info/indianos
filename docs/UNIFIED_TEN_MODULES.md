# IndianOS: consolidated ten-module kernel foundation

All ten previous `agents/NN-*` branches were confirmed identical to `main`:
failed model generations had not been committed. There was no feature source
on those branches to merge.

The ten numbered modules in `rust/kernel_core/src/` were implemented together
in one integration change. They are **bounded, allocation-free Rust policy
helpers with host-side unit tests**, not complete kernel subsystems:

| Agent | Module | Foundation |
|---|---|---|
| 01 | agent_01_fs_extents | Validated disk extents |
| 02 | agent_02_pkg_gate | Package-signature policy preflight |
| 03 | agent_03_cap_epoch | Owner/rights/epoch capabilities |
| 04 | agent_04_service_restart | Bounded restart state machine |
| 05 | agent_05_surface_rect | Owner-scoped rectangle clipping |
| 06 | agent_06_ipv4_route | IPv4 longest-prefix lookup |
| 07 | agent_07_pci_aperture | Safe PCI aperture arithmetic |
| 08 | agent_08_boot_slot | A/B boot-slot selection |
| 09 | agent_09_ci_policy | CI verdict and commit-bound gate |
| 10 | agent_10_app_policy | Signed app/catalog permission preflight |

No module accesses hardware, verifies cryptographic signatures, installs
applications, launches processes, or performs network I/O. Those require
separate runtime integrations and security reviews.

The former ten-matrix generative workflow
`.github/workflows/agent-swarm.yml` is now a **single read-only validation
job**. It checks all ten modules together after a successful full-kernel build
on main, using one host test pass and one freestanding kernel link.

For changes to any module, use a combined integration branch and full
`IndianOS kernel boot (x86-64)` pull-request CI before merging. A green
build does not establish runtime integration or security of unreviewed code.
