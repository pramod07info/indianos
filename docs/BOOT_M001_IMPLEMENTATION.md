# M001 — first independent-kernel implementation

## Implemented in source

- Higher-half x86-64 freestanding ELF kernel with Limine requests and protected load segments.
- Request Limine boot protocol base revision 6, memory map and optional 32-bit RGB framebuffer.
- Initialize COM1 serial output with bounded polling before using any interrupt handler.
- Validate nonempty memory map, bounds, overflow checks and presence of usable RAM.
- Render a small bounded tricolour graphic when a compatible framebuffer is provided.
- Halt safely with interrupts disabled. On invalid boot metadata, log a fatal error and halt.
- Build ISO with a *release-pinned, SHA256-verified* Limine binary; UEFI and BIOS boot paths.
- QEMU/OVMF smoke harness with ordered serial milestones and a timeout; GitHub CI workflow.

## Prerequisites (Linux or WSL2)

Install `make`, `clang`, `lld`, `binutils`, `curl`, `xorriso`, `qemu-system-x86`, `ovmf`, `python3` and `sha256sum` (GNU coreutils). The ISO build downloads the pinned Limine binary release over HTTPS and verifies its sha256 before extraction. Protocol definitions are vendored with their 0BSD license and pinned commit. The software does not contain a full OS installer.

## Usage

```bash
make toolchain-check
make kernel            # compile ELF; no download or QEMU required
make image             # downloads pinned Limine release; builds ISO
make test-boot         # boots UEFI QEMU and checks serial banner
make run               # interactive UEFI QEMU; Ctrl+C to quit
make run-bios          # optional legacy BIOS debugging
python3 -m unittest discover -s tests -v
```

If needed pass `OVMF_CODE=/path/to/OVMF_CODE_4M.fd` to `make run` or `make test-boot`. All disk images are created beneath `build/`; no host block device is accessed. **Never flash this to real hardware.**

## Expected serial output

```text
IndianOS: kernel entered
IndianOS: boot information validated
IndianOS: framebuffer status rendered
IndianOS: milestone 001 complete
```

When no supported framebuffer is present, the system emits an explanatory line and continues to milestone completion. A missing required memory map causes a fatal serial report and a safe halt, not a successful milestone.

## Dependency provenance

- Limine protocol header: https://github.com/limine-bootloader/limine-protocol, revision `3a0526b700e356f0eac1b71a77697b3fd1c707a3`, 0BSD.
- Limine bootloader release: https://github.com/Limine-Bootloader/Limine/releases/tag/v12.9.3, SHA256 of `limine-binary.tar.gz`: `9f42fe9ea2e84d71056529969b1d5c24ba9a2287ffcc01bcce58b0ef8aa98b1d`.
- Boot and linker layout follows public Limine C template examples but IndianOS kernel logic is project source.

## Limits and next step

The kernel has **no** own memory manager, scheduler, syscall ABI, process isolation, driver stack, userland, graphics compositor or application compatibility. Successful QEMU smoke testing must not be described as readiness to install on a PC. M002 should implement x86-64 exceptions (GDT/IDT) and a page frame allocator with separate emulator tests, without writing host disks.
