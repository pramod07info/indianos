# Boot contract and first-binary specification

This file is an **interface design**, not a description of working code.

## Target contract

- Platform: x86-64 QEMU + OVMF UEFI, ELF64 kernel.
- Early loader: Limine, pinned together with its compatible `limine.h` protocol header when coding starts.
- Entry: C-callable kernel entry with ABI required by the current Limine protocol; no assumption about a usable C runtime, heap, standard library or user processes.
- Required information: valid memory map; kernel should hard-fail cleanly if boot contract validation fails.
- Optional information for M001: framebuffer, HHDM, RSDP. Read only if corresponding response is present and valid. Do not dereference missing pointers.
- Output: COM1 (0x3F8) serial is the primary debug channel; framebuffer is opportunistic.

## BootInfo abstraction (conceptual)

```c
struct bootinfo {
    unsigned int version;
    const struct memory_region *regions;
    unsigned long region_count;
    void *framebuffer;        /* null if absent */
    unsigned long fb_width;
    unsigned long fb_height;
    unsigned long fb_pitch;
    unsigned int fb_bpp;
    unsigned long hhdm_offset; /* meaningful only with has_hhdm */
    bool has_hhdm;
};
```

In code, use fixed-width types and deliberate address/length conversions; the above is a boundary sketch, not an ABI header.

## Validation rules

1. Reject absent required responses, null arrays and impossible counts without iterating unsafe pointers.
2. Check framebuffer `pitch`, dimensions, pixel format and overflow before writing.
3. Treat the boot memory map as authoritative; exclude reserved and kernel regions from the free-frame pool.
4. Copy any metadata that must survive reclaiming bootloader memory.
5. Serial logging must not depend on framebuffer availability or dynamic allocation.
6. Keep interrupts controlled until exception handlers are installed.
7. Halt in a CPU loop after reporting fatal initialization failure.

## Boot chain

```text
OVMF/UEFI -> Limine -> IndianOS ELF entry -> serial/panic
                                    -> verify map
                                    -> optional framebuffer fill
                                    -> halt (M001)
```

We are not creating a custom bootloader in Milestone 001. A later loader experiment must preserve a documented boot interface and be tested independently.

## Planned verification

- UEFI QEMU boot produces the exact milestone serial banner.
- Running without a framebuffer does not crash the serial path.
- Invalid input is handled by a visible serial failure and safe halt.
- ELF inspection confirms x86-64 architecture and expected linker sections.
