# IndianOS kernel P0 batch 6 — P0-101 through P0-120

Continuation on draft PR #786, parent `e26d3f0f41b25aa25310dd0210cd5740786fb217`. This batch adds real kernel library functions and negative/positive tests in six existing kernel areas. W1–W6 are GitHub Actions **verification jobs**, NOT autonomous development agents.

| ID | Workstation | Source implementation / negative condition |
| --- | --- | --- |
| P0-101 | W1 | Verify complete 1–64 expected AP topology before reclaim |
| P0-102 | W1 | Reject missing or unlisted AP receipts |
| P0-103 | W1 | Reject duplicate expected AP IDs and impossible CPU identities |
| P0-104 | W1 | Reject stale, pending or foreign-BSP reclaim receipts |
| P0-105 | W2 | Gate atomic process exit on exact required AP topology |
| P0-106 | W2 | Preserve queues and exit notices after pending AP proof |
| P0-107 | W2 | Reject wrong AP inventory without retiring a process |
| P0-108 | W2 | Roll back both queues when notification capacity is exhausted |
| P0-109 | W3 | Enforce exact-generation relay recipient allowlist |
| P0-110 | W3 | Reject unlisted or wrong-generation targets before consuming FIFO |
| P0-111 | W3 | Reject ambiguous duplicate PID authorizations |
| P0-112 | W3 | Preserve authenticated relay attribution and original message FIFO |
| P0-113 | W4 | Bound unaligned metadata to an authorized partition sector span |
| P0-114 | W4 | Reject sector overflow or partition escape before device I/O |
| P0-115 | W4 | Retire metadata view on late I/O fault without partial buffer changes |
| P0-116 | W5 | Validate Ethernet IPv4 header length, total length and header checksum |
| P0-117 | W5 | Reject IPv4 fragments and RX epoch races; exclude Ethernet padding |
| P0-118 | W6 | Bind privileged launch to independently verified signed image digest |
| P0-119 | W6 | Deny invalid/missing manifest image match before PID1 launch callback |
| P0-120 | W6 | Consume trusted launch receipt on image-verifier failure to prevent replay |

## Verification

The exact-head `kernel-p0-six-workstations.yml` adds new `kernel_p0_batch6_w1..w6` regression filters to the existing five test families; each W1–W6 job also compiles the freestanding no_std Rust feature profile. All CI status checks, QEMU SMP/boot/persistence evidence, security gate, independent review, and branch protection must be verified **at this new head**. No pass is asserted by this document.

## Explicit acceptance boundaries

All 20 entries are **source-level implementation/test items**, not 20 completed production hardware acceptance gates. Physical APIC IPI and TLB shootdown, NIC IRQ/DMA/IOMMU quiescence, trusted TPM/UEFI and signed manifest source, live PID1 executable loader, and destructive power-loss recovery are still OPEN. Production users must provide authenticated topology and manifest inputs under the required kernel lock; these methods do not manufacture those guarantees. Keep PR draft and do not merge until independent review and required checks pass.

Additional topology regression: APIC ID 0 is legal for an AP when the BSP has a different APIC ID; identity validation checks ownership, not a blanket nonzero APIC-ID rule.
