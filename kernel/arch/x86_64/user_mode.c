#include <indianos/user_mode.h>
#include <indianos/boot.h>
#include <indianos/interrupts.h>
#include <indianos/paging.h>
#include <indianos/paging_clone.h>
#include <indianos/pmm.h>
#include <indianos/user_image.h>
#include <indianos/syscall_abi.h>
#include <indianos/serial.h>
#include <stdint.h>
#include <stddef.h>

#define PHYS_MASK 0x000ffffffffff000ull
#define PAGE 4096ull

extern uint64_t x86_64_enter_user(uint64_t entry, uint64_t user_sp,
                                   uint64_t process_cr3);
extern __attribute__((noreturn)) void x86_64_user_exit_to_kernel(uint64_t status);

struct direct_memory { uint64_t offset; };
static struct indianos_user_image *current_user;

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

/* Copy page-at-a-time through the kernel HHDM after checking the PLANNED
 * bounds AND each actual leaf. Never dereference the user VA in ring 0. */
bool indianos_user_copy_readonly(const struct indianos_user_image *image,
                                 uint64_t address, void *destination, size_t length) {
    if (image == NULL || !image->ready || destination == NULL ||
        length == 0 || length > INDIANOS_MAX_SYSCALL_WRITE ||
        !indianos_program_user_range(&image->plan, address, length, false))
        return false;
    uint8_t *out = destination;
    size_t done = 0;
    while (done < length) {
        const uint64_t at = address + done;
        const uint64_t page_va = at & ~(PAGE - 1);
        const size_t offset = (size_t)(at & (PAGE - 1));
        struct vmm_mapping mapping = {0};
        if (!vmm_query_page(&image->tables, page_va, &mapping) ||
            !mapping.permissions.user) return false;
        const uint8_t *source = image->tables.backend.access(
            mapping.physical, image->tables.backend.context);
        if (source == NULL) return false;
        size_t count = PAGE - offset;
        if (count > length - done) count = length - done;
        for (size_t i = 0; i < count; ++i) out[done + i] = source[offset + i];
        done += count;
    }
    return true;
}

/* Register save frame emitted by user_entry.S; indexes are fixed to ASM. */
uint64_t x86_64_user_syscall_dispatch(const uint64_t *regs) {
    if (current_user == NULL || regs == NULL)
        x86_64_user_exit_to_kernel(127);
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
    return paging_lookup(b, source, address, &a, &af) &&
           paging_lookup(b, clone, address, &bpa, &bf) &&
           a == bpa && (af & 1ull) && (bf & 1ull);
}

bool x86_64_user_demo_run(const uint8_t *elf, size_t size) {
    struct paging_cpu_state cpu = {0};
    uint64_t offset = 0;
    if (!x86_64_paging_detect(&cpu) || !paging_preflight(&cpu) ||
        cpu.pcid_enabled || !boot_hhdm_offset(&offset) || offset == 0 ||
        !x86_64_user_interrupt_stack_enable()) return false;

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
    if (!indianos_user_image_load(&staged, &vmm, elf, size)) return false;

    if (paging_clone_kernel_half(&clone_backend, original, &clone) &&
        paging_attach_user_half(&clone_backend, clone.root, staged.tables.root_physical)) {
        grafted = true;
        uint64_t stack = 0;
        __asm__ volatile ("mov %%rsp, %0" : "=r"(stack));
        const uint64_t test_addresses[] = {
            (uint64_t)(uintptr_t)&x86_64_user_demo_run,
            (uint64_t)(uintptr_t)&current_user,
            (uint64_t)(uintptr_t)&x86_64_user_syscall_dispatch,
            stack,
        };
        bool safe = true;
        for (unsigned i = 0; i < sizeof(test_addresses)/sizeof(test_addresses[0]); ++i) {
            if (!valid_kernel_access(&clone_backend, original, clone.root,
                                     test_addresses[i])) safe = false;
        }
        uint64_t entry_pa = 0, entry_flags = 0;
        uint64_t guard_pa = 0, guard_flags = 0;
        if (!paging_lookup(&clone_backend, clone.root, staged.plan.entry,
                           &entry_pa, &entry_flags) ||
            !paging_lookup(&clone_backend, clone.root, staged.plan.stack_end - 16,
                           &guard_pa, &guard_flags) ||
            paging_lookup(&clone_backend, clone.root, staged.plan.guard_begin,
                          &guard_pa, &guard_flags)) safe = false;
        if (safe) {
            current_user = &staged;
            serial_write("IndianOS: entering ring-3 /bin/init (QEMU experimental)\n");
            /* Restore CR3 on SYS_EXIT before touching/freeing borrowed roots. */
            const uint64_t status = x86_64_enter_user(
                staged.plan.entry, staged.plan.stack_end - 16, clone.root);
            current_user = NULL;
            success = (status == 0);
            if (success) serial_write("IndianOS: /bin/init exited through int 0x80\n");
        }
    }
    /* The x86-64 exit trampoline has already restored the original CR3.
     * Detach borrowed user children before releasing cloned kernel tables. */
    if (grafted && !paging_detach_user_half(&clone_backend, clone.root,
                                            staged.tables.root_physical))
        return false; /* Do not risk a double-free if invariants are broken. */
    if (clone.root != 0 && !paging_clone_release(&clone_backend, &clone)) return false;
    if (!indianos_user_image_destroy(&staged)) return false;
    return success && pmm_free_frames() == starting_free;
}
