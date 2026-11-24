# Normal Desktop interrupt-driven input

The source audit at `7b2184749db056bf4d19e3f1c3d162179fc71440`
found normal Desktop still polling the 8042 controller, while the isolated
IRQ1 keyboard profile already provided a decoder and bounded keyboard queue.
This change extends that path rather than adding a second keyboard decoder.

Normal Desktop now installs IRQ1/IRQ12 gates, admits IF in native ring-3
entry/resume policy and enables only those PIC inputs plus the slave cascade.
The existing local-APIC virtual-wire setup supplies the legacy PIC route;
unused supported LVT entries are masked, and the spurious vector returns
without an EOI. The MMIO mapping is supervisor-only with PCD/PWT. Its paging
frames remain kernel-owned until reboot, including initialization failure.
This is a single-BSP legacy-PIC implementation, not SMP or IOAPIC support.

Mouse initialization quiesces both device clocks before discarding bounded
stale controller bytes. After device commands are acknowledged, the shared
controller command byte enables both input IRQs. The IRQ handlers save all
GPRs, drain at most32 transfers and route each byte using its AUX status.
IRQ12 acknowledges the slave before the master. Routing both channels from
either handler avoids stranding a keyboard byte behind a pending AUX byte.
No ISR performs scheduling, application policy, graphics or filesystem I/O.

The keyboard uses the existing loss-cancellation queue. Mouse packets retain
button edges in a bounded queue. Corruption, movement overflow or queue
overflow discards pending motion and publishes a zero-motion release so stale
capture cannot survive; held-button packets are quarantined until physical
release. Normal syscall consumers read queues only, with IF masked. Legacy
experimental mouse profiles keep their established polling path.

Session retirement masks both PICs and clears input queues before controller
commands. Replacement sessions discard old bytes, reset decoder/queue state
and reset both vector and routed-data evidence counters. Recovery remains
interrupt-masked and uses its separate UART path.

Actual guest evidence requires all four records after device input:

- `IndianOS: G5 normal IRQ1 delivered`
- `IndianOS: G5 normal IRQ12 delivered`
- `IndianOS: G5 normal IRQ keyboard data queued`
- `IndianOS: G5 normal IRQ mouse packet queued`

Vector records alone are insufficient: the data records require a decoded
make byte or complete mouse packet inside an ISR. Existing graphical focus,
capture, Files/Terminal changes, UART rejection and reboot acceptance remain
required. New host tests cover mixed-channel routing, bounded draining,
packet-edge preservation and cancellation/quarantine after overflow or a bad
transfer. They were authored without local execution, as requested; exact-head
GitHub host/no_std and real QEMU evidence are required before acceptance.
Physical-machine qualification and hostile CPU availability remain separate.
