# IndianOS R118 inherited gates — cohort 4

The following are **still pending**, not delivered as physical platform functionality. The new Rust tests exercise cooperative/state-machine cases only.

| ID | Target | Required evidence before checking |
|---|---|---|
| 071 | Concurrent mount replacement | Two actual AP/BSP workers race remount and path lookup with kernel lock traces |
| 072 | Physical IRQ port-generation admission | NIC IRQ handler captures exact port-owner generation, including after PID reuse |
| 073 | File/network lease lock order | Static and dynamic nested lock-order audit under preemption |
| 074 | No SMP scheduler teardown deadlock | Real AP worker fault/exit + concurrent peer network/file operations |
| 075 | Production boot PID1 trust observer | Measured firmware value is actually passed from privileged boot chain |
| 076 | Immutable trust root | UEFI/TPM backed sealed root survives reboot; tampering denied |
| 077 | Abrupt power-cut journal validation | Repeat real unsafe cut at each journal write/flush transition |
| 078 | Boot-sector repair on physical disk | Signed-image recovery and sector readback on a sacrificial test device |
| 079 | Hardware-clock transmit checks | Calibrated timer source before and after NIC buffer construction, IRQ retry |
| 080 | Multiple NIC IOMMU isolation | Physical PCI DMA remap and error-injection proof across two adapters |

**New code evidence:** unit tests for stale namespace and mount revision after a cooperative state change; generation-bound UDP registry retirement and PID reuse. **Not proven:** inter-CPU atomicity, IRQ serialization, IOMMU, immutable firmware root, actual unsafe power loss.

Do not replace hardware evidence with checkboxes or QEMU-only logs.
