# R118 release evidence classification (cohort 3)

## Kernel QA vs physical qualification

| Gate | Evidence needed | Current source-supported status |
| --- | --- | --- |
| Boot-sector restoration (056) | Unplug/reconnect power, physical media inspection and reboot logs | OPEN; no physical device controlled |
| Optimized no_std (062) | exact head freestanding optimized CI build | OPEN until this head CI |
| Rust host tests (063) | exact head complete host suite, not merely selected tests | OPEN until verified |
| Persistent graphical Files (064) | two distinct cold boots with matching file checksum | OPEN until raw logs inspected |
| Ring3 UDP QEMU (065) | boot serial proof of both actual user tasks, exact generation, port ownership | OPEN |
| Network and boot matrix (066) | exact SHA QEMU ingress, epoch and recovery jobs | OPEN |
| Independent review (067) | distinct approver, not a workflow named review | OPEN |
| Exact integration SHA (068) | all required checks on final commit hash | OPEN until final commit |
| Evidence scope (069) | this matrix and release tool separate hardware vs QEMU | DOCUMENTED |
| Protected merge (070) | independently reviewed protected merge, post-merge main SHA | OPEN |

The release verifier in scripts/r118_release_gate.py fails closed on missing exact-head checks, independent review or branch protection. It does **not** prove real hardware: SMP, IOMMU, physical NIC IRQs, immutable TPM/UEFI trust anchor, unsafe power-cut, and calibrated monotonic clock are outside GitHub Actions QEMU evidence.
