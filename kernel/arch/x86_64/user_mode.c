#include <indianos/user_mode.h>
#include <indianos/boot.h>
#include <indianos/interrupts.h>
#include <indianos/paging.h>
#include <indianos/paging_clone.h>
#include <indianos/pmm.h>
#include <indianos/user_image.h>
#include <indianos/syscall_abi.h>
#include <indianos/user_fault.h>
#include <indianos/serial.h>
#include <stdint.h>
#include <stddef.h>

#define PHYS_MASK 0x000ffffffffff000ull
#define PAGE 4096ull

extern void x86_64_user_interrupt(void);
extern uint64_t x86_64_enter_user(uint64_t entry, uint64_t user_sp,
                                   uint64_t process_cr3);
extern __attribute__((noreturn)) void x86_64_user_exit_to_kernel(uint64_t status);

struct direct_memory { uint64_t offset; };
static struct indianos_user_image *current_user;
static uint64_t last_user_fault_vector;
static uint64_t last_user_fault_error;
static uint64_t last_user_fault_address;

/* Called by the CPU exception dispatcher. No caller may access stale user
 * memory after this: the x86 trampoline restores kernel CR3 and original
 * caller stack before returning from x86_64_enter_user. */
void x86_64_user_fault_handle(uint64_t vector, uint64_t cs,
                              uint64_t error_code, uint64_t cr2) {
    if (!indianos_user_fault_eligible(vector, cs, error_code,
                                      current_user != NULL)) {
        return; /* Kernel faults must continue into terminal fault handler. */
    }
    last_user_fault_vector = vector;
    last_user_fault_error = error_code;
    last_user_fault_address = cr2;
    if (vector == INDIANOS_USER_VECTOR_PF) {
        serial_write("IndianOS: isolated ring-3 page fault\n");
    } else if (vector == INDIANOS_USER_VECTOR_GP) {
        serial_write("IndianOS: isolated ring-3 general protection fault\n");
    } else {
        serial_write("IndianOS: isolated ring-3 invalid opcode\n");
    }
    x86_64_user_exit_to_kernel(INDIANOS_USER_FAULT_EXIT_BASE + vector);
}


static bool physical_in_boot_map(uint64_t frame) {
    if (frame == 0 || (frame & (PAGE - 1)) || frame > UINT64_MAX - PAGE) return false;
    for (uint64_t i = 0; i < boot_memory_region_count(); ++i) {
        struct indianos_memory_region m = {0};
        if (!boot_memory_region_at(i, &m, NULL) ||
            m.base > UINT64_MAX - m.length) return false;
        if (frame >= m.base && frame - m.base <= m.length &&
            m.length - (frame - m.base) >= PAGE) {
            return m.type == 0 || m.type == 5 || m.type == 6 || m.type == 8;
        }
    }
    return false;
}

static bool frame_allocate(uint64_t *frame, void *context) {
    (void)context;
    return pmm_allocate_frame(frame);
}
static bool frame_release(uint64_t frame, void *context) {
    (void)context;
    return pmm_release_frame(frame);
}
static void *frame_access(uint64_t frame, void *context) {
    const struct direct_memory *ctx = context;
    if (ctx == NULL || !physical_in_boot_map(frame) ||
        frame > UINT64_MAX - ctx->offset) return NULL;
    const uint64_t address = frame + ctx->offset;
    if ((address >> 48) != 0xffffull || (address & (1ull << 47)) == 0)
        return NULL;
    return (void *)(uintptr_t)address;
}
static uint64_t *table_access(uint64_t frame, void *context) {
    return (uint64_t *)frame_access(frame, context);
}

/* Register save frame emitted by user_entry.S; indexes are fixed to ASM. */
uint64_t x86_64_user_syscall_dispatch(const uint64_t *regs) {
    /* No active user process means no initialized return state. Never
     * attempt to restore a zero/stale saved CR3 in this situation. */
    if (current_user == NULL || regs == NULL)
        return UINT64_MAX;
    const struct indianos_syscall_request request = {
        .number = regs[14], .arg0 = regs[9], .arg1 = regs[10],
    };
    if (!indianos_syscall_validate(&current_user->plan, request))
        return UINT64_MAX;
    if (request.number == INDIANOS_SYS_ABI_QUERY)
        return INDIANOS_SYSCALL_ABI;
    if (request.number == INDIANOS_SYS_EXIT)
        x86_64_user_exit_to_kernel(request.arg0);
    if (request.number == INDIANOS_SYS_WRITE) {
        char copied[INDIANOS_MAX_SYSCALL_WRITE + 1];
        if (!indianos_user_copy_readonly(current_user, request.arg0,
                                         copied, (size_t)request.arg1))
            return UINT64_MAX;
        copied[request.arg1] = '\0';
        serial_write("IndianOS user: ");
        serial_write(copied);
        serial_write("\n");
        return request.arg1;
    }
    return UINT64_MAX;
}

static bool valid_kernel_access(const struct paging_clone_backend *b,
                                uint64_t source, uint64_t clone, uint64_t address) {
    uint64_t a = 0, bpa = 0, af = 0, bf = 0;
    if ((address & (1ull << 47)) == 0) return false;
    const uint64_t *root = b->access(clone, b->context);
    const unsigned int top_index = (unsigned int)((address >> 39) & 511ull);
    if (root == NULL || top_index < 256 ||
        (root[top_index] & 0x5ull) != 0x1ull) {
        /* Present supervisor PML4 is required for all kernel structures. */
        return false;
    }
    return paging_lookup(b, source, address, &a, &af) &&
           paging_lookup(b, clone, address, &bpa, &bf) &&
           a == bpa && (af & 1ull) && (bf & 1ull);
}

bool x86_64_user_demo_run(const uint8_t *elf, size_t size) {
    struct paging_cpu_state cpu = {0};
    uint64_t offset = 0;
    if (!x86_64_paging_detect(&cpu)) {
        serial_write("IndianOS: ring-3 preflight failed: CPU paging probe\n");
        return false;
    }
    if (!paging_preflight(&cpu)) {
        serial_write("IndianOS: ring-3 preflight failed: NXE/WP/PAE/LA57 or CR3\n");
        return false;
    }
    if (cpu.pcid_enabled) {
        serial_write("IndianOS: ring-3 preflight failed: unsupported PCID mode\n");
        return false;
    }
    if (!boot_hhdm_offset(&offset) || offset == 0) {
        serial_write("IndianOS: ring-3 preflight failed: missing HHDM\n");
        return false;
    }
    if (!x86_64_user_interrupt_stack_enable()) {
        serial_write("IndianOS: ring-3 preflight failed: TSS RSP0\n");
        return false;
    }
    serial_write("IndianOS: ring-3 paging prerequisites verified\n");

    struct direct_memory memory = {.offset = offset};
    struct vmm_backend vmm = {
        .allocate = frame_allocate, .release = frame_release,
        .access = frame_access, .context = &memory,
    };
    struct paging_clone_backend clone_backend = {
        .allocate = frame_allocate, .release = frame_release,
        .access = table_access, .context = &memory,
    };
    const uint64_t original = cpu.cr3 & PHYS_MASK;
    const uint64_t starting_free = pmm_free_frames();
    struct indianos_user_image staged = {0};
    struct paging_clone_result clone = {0};
    bool grafted = false;
    bool success = false;
    if (!indianos_user_image_load(&staged, &vmm, elf, size)) {
        serial_write("IndianOS: ring-3 failed: ELF segment staging\n");
        return false;
    }
    serial_write("IndianOS: ring-3 user image staged\n");

    const bool cloned = paging_clone_kernel_half(&clone_backend, original, &clone);
    if (!cloned) {
        serial_write("IndianOS: ring-3 failed: supervisor mapping clone\n");
    }
    if (cloned && paging_attach_user_half(
            &clone_backend, clone.root, staged.tables.root_physical)) {
        serial_write("IndianOS: ring-3 user page tables attached\n");
        grafted = true;
        uint64_t stack = 0;
        __asm__ volatile ("mov %%rsp, %0" : "=r"(stack));
        uint64_t trap_addresses[INDIANOS_TRAP_MAPPING_COUNT] = {0};
        bool safe = x86_64_user_trap_mapping_addresses(trap_addresses);
        const uint64_t test_addresses[] = {
            (uint64_t)(uintptr_t)&x86_64_user_demo_run,
            (uint64_t)(uintptr_t)&x86_64_enter_user,
            (uint64_t)(uintptr_t)&x86_64_user_interrupt,
            (uint64_t)(uintptr_t)&x86_64_user_exit_to_kernel,
            (uint64_t)(uintptr_t)&x86_64_user_syscall_dispatch,
            (uint64_t)(uintptr_t)&current_user,
            stack,
        };
        for (unsigned i = 0; i < INDIANOS_TRAP_MAPPING_COUNT; ++i) {
            if (!valid_kernel_access(&clone_backend, original, clone.root,
                                     trap_addresses[i])) safe = false;
        }
        for (unsigned i = 0; i < sizeof(test_addresses)/sizeof(test_addresses[0]); ++i) {
            if (!valid_kernel_access(&clone_backend, original, clone.root,
                                     test_addresses[i])) safe = false;
        }
        uint64_t entry_pa = 0, entry_flags = 0;
        uint64_t stack_pa = 0, stack_flags = 0;
        uint64_t guard_pa = 0, guard_flags = 0;
        if (!paging_lookup(&clone_backend, clone.root, staged.plan.entry,
                           &entry_pa, &entry_flags) ||
            !paging_lookup(&clone_backend, clone.root, staged.plan.stack_end - 16,
                           &stack_pa, &stack_flags) ||
            paging_lookup(&clone_backend, clone.root, staged.plan.guard_begin,
                          &guard_pa, &guard_flags) ||
            (entry_flags & 0x7ull) != 0x5ull ||
            (entry_flags & (1ull << 63)) != 0 ||
            (stack_flags & 0x7ull) != 0x7ull ||
            (stack_flags & (1ull << 63)) == 0) {
            safe = false;
        }
        if (!safe) {
            serial_write("IndianOS: ring-3 mapping preflight rejected unsafe address space\n");
        }
        if (safe) {
            last_user_fault_vector = 0;
            last_user_fault_error = 0;
            last_user_fault_address = 0;
            current_user = &staged;
            serial_write("IndianOS: entering ring-3 /bin/init (QEMU experimental)\n");
            /* SYS_EXIT or eligible user fault restores CR3 and caller RSP.
             * Never free borrowed page tables before that return. */
            const uint64_t status = x86_64_enter_user(
                staged.plan.entry, staged.plan.stack_end - 16, clone.root);
            current_user = NULL;
#if defined(INDIANOS_TEST_USERFAULT_GUARD)
            const uint64_t expected_address =
                INDIANOS_USER_TOP - INDIANOS_USER_STACK_BYTES - INDIANOS_USER_PAGE_BYTES;
            const bool expected_present = false;
#elif defined(INDIANOS_TEST_USERFAULT_SUPERVISOR)
            const uint64_t expected_address = 0xffffffff80000000ull;
            const bool expected_present = true;
#endif
#if defined(INDIANOS_TEST_USERFAULT_GUARD) || defined(INDIANOS_TEST_USERFAULT_SUPERVISOR)
            success = (status == INDIANOS_USER_FAULT_EXIT_BASE + INDIANOS_USER_VECTOR_PF) &&
                      (last_user_fault_vector == INDIANOS_USER_VECTOR_PF) &&
                      (last_user_fault_address == expected_address) &&
                      ((last_user_fault_error & 0x4ull) != 0) &&
                      (((last_user_fault_error & 0x1ull) != 0) == expected_present) &&
                      ((last_user_fault_error & 0x8ull) == 0);
            if (success) {
                serial_write("IndianOS: user page fault contained; kernel resumed\n");
            } else {
                serial_write("IndianOS: user page fault validation failed\n");
            }
#else
            success = (status == 0 && last_user_fault_vector == 0);
            if (success) serial_write("IndianOS: /bin/init exited through int 0x80\n");
#endif
        }
    }
    /* The x86-64 exit trampoline has already restored the original CR3.
     * Detach borrowed user children before releasing cloned kernel tables. */
    if (grafted && !paging_detach_user_half(&clone_backend, clone.root,
                                            staged.tables.root_physical)) {
        serial_write("IndianOS: ring-3 cleanup failed: user PML4 detach\n");
        return false; /* Do not risk a double-free if invariants are broken. */
    }
    serial_write("IndianOS: ring-3 borrowed user tables detached\n");
    if (clone.root != 0 && !paging_clone_release(&clone_backend, &clone)) {
        serial_write("IndianOS: ring-3 cleanup failed: kernel clone release\n");
        return false;
    }
    serial_write("IndianOS: ring-3 supervisor clone released\n");
    if (!indianos_user_image_destroy(&staged)) {
        serial_write("IndianOS: ring-3 cleanup failed: user image destroy\n");
        return false;
    }
    serial_write("IndianOS: ring-3 user image released\n");
    const uint64_t after = pmm_free_frames();
    if (after != starting_free) {
        serial_write("IndianOS: ring-3 cleanup failed: PMM frame count mismatch\n");
        return false;
    }
    serial_write("IndianOS: ring-3 PMM frame count restored\n");
    return success;
}
