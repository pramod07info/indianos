# Milestone 004 — paging preflight, TSS/IST, guarded CR3 and page-fault diagnostics

**Status: source implemented and host-tested; QEMU/OVMF execution not independently verified locally.** IndianOS is not yet an installable operating system.

## Implemented

1. **CPU paging detection:** CPUID, CR0, CR3, CR4 and EFER; a separately tested pure preflight policy requires long mode, NX/MSR/PAE, enabled paging and NXE, CR0.WP, physical-address width 36–52 bits, a valid CR3 root and four-level paging (LA57 disabled). The default boot does not change CPU control settings.
2. **TSS/IST:** A 64-bit Task State Segment descriptor in the GDT is loaded via the processor task register. Double fault (#DF, vector 8) uses IST1 and a dedicated 16 KiB emergency stack. There is no ring-3 RSP0 policy or stack guard page.
3. **Page fault reporting:** Fatal vector 14 (#PF) now reports CR2, RIP, the error code and interpreted present/write/user/reserved/instruction bits over COM1.
4. **Bounded shadow clone:** The current four-level tree can be copied into independent PMM-owned page-table frames, preserving 4 KiB, 2 MiB and 1 GiB leaf mappings. Source frames are never modified, page-table allocations are bounded to 8,192 frames, and failed allocation rolls back. Mapped data frames remain owned by their original owners.
5. **Test-only CR3 round trip:** A separately built PAGING_SWITCH_SELFTEST=1 variant checks that code, stack, data and HHDM root translations are identical, then can temporarily load the cloned CR3, immediately restore the original and reclaim cloned table frames. This is OFF in the normal kernel and has NOT been QEMU verified.

## Tests and build commands

On Linux/WSL with Clang, LLD, binutils, make and Python 3:

    make kernel
    make test-host

With xorriso, QEMU, OVMF, curl and the pinned Limine loader:

    make image
    make test-boot
    make test-exception
    make test-pagefault
    make test-shadow    # EXPERIMENTAL QEMU-only manual smoke test

GitHub Actions on pushes to main runs host tests, a QEMU boot test, the UD2 exception variant and the intentional page-fault variant. The shadow CR3 smoke test is separately opt-in through workflow_dispatch with run_shadow_switch enabled.

Expected new normal serial events:

    IndianOS: paging capabilities inspected
    IndianOS: shadow paging preflight passed

If preflight fails, the second message becomes IndianOS: shadow paging switch not permitted. Both outcomes leave CR3 unchanged in normal boot. Successful inspection then emits:

    IndianOS: milestone 004 complete

The deliberate page fault variant reports:

    IndianOS: triggering intentional page fault
    IndianOS: FATAL CPU exception vector=0x000000000000000e ...
    IndianOS: page fault cr2=0x00007fff00000000 present=0 write=0 ...

The optional CR3 test emits IndianOS: shadow CR3 switched and restored only if the guarded switch and restoration succeed.

## Verification performed locally

- Default, intentional page-fault and opt-in CR3-switch kernel variants compile as x86-64 freestanding ELF binaries under Clang/LLD.
- PMM, VMM and paging clone/preflight C host harnesses pass.
- 12 Python QEMU transcript parser tests pass.
- The paging clone/preflight C harness passes AddressSanitizer and UndefinedBehaviorSanitizer.
- ELF readelf inspection confirms distinct mapped R/W/X segments.
- QEMU/OVMF runtime was NOT run because QEMU, OVMF and xorriso are absent locally.

## Security and limitations

The cloned root inherits loader mappings (kernel, direct map, firmware), so it does NOT isolate user processes. The optional CR3 smoke test is one-CPU and interrupts-off; its PCID/global TLB, firmware, DMA and page-attribute assumptions require more evidence before use in a permanent kernel address space. A TSS emergency stack is not a fully guarded kernel stack. Default boot never switches CR3, never silently enables NXE/WP, and never runs a third-party application. The experimental ISO must not be used on a real PC or physical drive.

## Next: M005

Qualify a permanently owned kernel root and enforce ownership for process/kernel pages; add TSS RSP0 policy, guarded stacks, ELF user-process loader, minimal read-only initramfs and typed syscall ABI. User/kernel privilege isolation must precede application compatibility work.
