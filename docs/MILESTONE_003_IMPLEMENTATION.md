# Milestone 003 — detached virtual memory manager (x86-64)

**Status:** Freestanding source and host tests implemented. Builds into the kernel ELF. Full Limine/OVMF QEMU execution has **not** been independently verified locally, and this is **not** an active process address space.

## Architecture and boundaries

- A generic, freestanding page-table engine creates **detached four-level** x86-64 PML4/PDP/PD/PT trees from physical frames. It never loads CR3, touches active page tables, invalidates TLB entries, or accesses a virtual address merely because it mapped it in a detached tree.
- Explicit `vmm_backend` callbacks allocate, release and access physical page-table frames. The kernel backend uses M002's conservative PMM for owned table frames and Limine's requested HHDM response to zero and inspect those frames safely.
- No direct physical address dereferences or assumptions that a physical frame equals a virtual address. Missing HHDM is a fatal M003 self-test failure rather than unsafe memory access.
- The detached tree maps **one** test data frame at a high, canonical address (`0xfffffe0000000000`) and checks the mapping metadata. It then unmaps and returns all data/table frames, verifying the PMM free count is unchanged.
- A host-side C test uses the **same VMM engine** with RAM-backed fake frames, including failure injection and rollback. No privileged instructions are used in that host test.

## Memory permissions and policy

- 4 KiB page mapping only; no huge pages, five-level paging, shared address spaces, copy-on-write, swapping, or process switching.
- User pages are accepted only in the low canonical half; kernel pages only in the high canonical half. Intermediate user/supervisor privilege mismatches are rejected.
- `writable && executable` is prohibited (W^X policy); non-executable leaf entries carry the x86-64 NX bit.
- **Important:** These are **detached**, unactivated page tables. Before ever loading CR3, IndianOS must check CPU NX capability, enable `EFER.NXE`, construct/copy valid firmware/kernel/direct-map mappings, choose 4-level/5-level paging safely, establish interrupt stacks, and provide TLB shootdown/flush semantics. M003 does none of that and must **never** load this detached root.
- The caller owns mapped **data** frames; the VMM owns **page-table** frames only. A successful unmap does not release data frames.
- Map requests are non-replacing, rejecting a duplicate; partial table creation rolls back allocated frames. Empty intermediate tables are freed when unmapping the last leaf; `vmm_destroy_empty` refuses to discard a non-empty space.
- Single CPU, no interrupts enabled, no locking, and no security claim for future user applications yet.

## Host validation

```sh
make kernel
make test-host
# Optional memory safety test for development hosts:
clang -fsanitize=address,undefined -fno-omit-frame-pointer -O1 -g \
  -std=c11 -Wall -Wextra -Werror -I kernel/include \
  tests/test_vmm.c kernel/src/vmm.c -o /tmp/vmm_sanitized
/tmp/vmm_sanitized
```

Expected new serial lines after the M002 success marker:

```text
IndianOS: detached virtual memory manager ready
IndianOS: milestone 003 complete
```

Missing HHDM, out-of-memory or mapping failure instead emits:

```text
IndianOS: FATAL virtual memory self-test failed
```

For a complete boot test, QEMU/OVMF/xorriso and pinned Limine binaries are required:

```sh
make image
make test-boot
make test-exception
```

## Verification performed (local)

- Freestanding `x86_64-unknown-none-elf` ELF kernel compiled with Clang/LLD.
- Separate deliberate-`UD2` ELF also compiled.
- Production VMM implementation passed native Clang and GCC C unit harnesses.
- VMM host harness passed Clang ASan/UBSan checks.
- The PMM host tests and eight Python serial transcript tests passed.
- **Not run:** actual QEMU boot/UEFI handoff, physical hardware, or loading the new page-table root into CR3. This kernel remains a research prototype.

## Next bounded milestone — M004

- Privileged paging capability detection and safe transition strategy: bootstrap a usable higher-half root, preserve existing mappings, audit NXE/WP/SMEP/SMAP/PAT state and switch CR3 only after robust platform-specific tests.
- Extend exception tables to report CR2 on page faults, build TSS/IST for double faults, and add a deliberate missing-page test.
- Design process memory ownership, guarded stack mappings, address-space copy and `copyin/copyout` before implementing ring 3.
- Continue never testing early kernels on real boot disks.
