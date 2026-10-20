# Driver and hardware strategy

## Ordering principles

Only target **emulated hardware** at first. Real device support requires device documentation, DMA safety, interrupt safety and a reproducible test matrix.

| Order | Device/system | First goal | Notes |
| --- | --- | --- | --- |
| 1 | x86-64 COM1 serial | Early logging | Polling; no heap/interrupt dependency |
| 2 | Boot framebuffer | Draw solid test colour | Limine response; no accelerated GPU |
| 3 | CPU exceptions / timer | Reliable faults and ticks | GDT/IDT, PIC/APIC plan |
| 4 | ACPI + PCI discovery | Enumerate buses | Parse untrusted tables defensively |
| 5 | PS/2 keyboard (QEMU) | Input events | Emulator-first |
| 6 | virtio-blk | Block reads | Read-only in early bring-up |
| 7 | initramfs and simple VFS | Load a user program | VFS abstraction before writable storage |
| 8 | virtio-net | Basic packets | Network stack and isolation required |
| 9 | xHCI USB / NVMe / ACPI power | Broader PC support | Hardware matrix and fault containment |
| 10 | Vendor GPUs, Wi-Fi, audio | Real laptop experience | Multi-year driver effort |

## Proposed driver interface

```text
device discovery -> matching / probe -> resources -> init
                                   -> irq/dma setup
                                   -> start / stop / reset
```

Keep an abstract device record with bus identity, resources, ownership and state. Drivers must have explicit error returns, no uncontrolled memory access, and a documented interrupt/DMA policy. PCI devices must not be bus-master enabled until DMA memory policy is defined.

## Desktop input/display contracts

The kernel provides safe primitives, event queues, buffers and permissions. Compositor/window management stays **in user space**. Avoid embedding the desktop UI in a device driver.

## Test approach

QEMU devices first; negative paths (missing devices, bad capability registers, timeouts) must be testable. Later hardware support will use a named list of certified test machines rather than claiming generic Windows-driver compatibility.
