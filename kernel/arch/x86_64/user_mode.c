#include <indianos/user_mode.h>
#include <indianos/boot.h>
#include <indianos/interrupts.h>
#include <indianos/paging.h>
#include <indianos/paging_clone.h>
#include <indianos/pmm.h>
#include <indianos/user_image.h>
#include <indianos/syscall_abi.h>
#include <indianos/user_fault.h>
#include <indianos/process.h>
#include <indianos/scheduler.h>
#include <indianos/timer.h>
#include <indianos/serial.h>
#include <stdint.h>
#include <stddef.h>

#define PHYS_MASK 0x000ffffffffff000ull
#define PAGE 4096ull

extern void x86_64_user_interrupt(void);
extern uint64_t x86_64_enter_user(uint64_t entry, uint64_t user_sp,
                                   uint64_t process_cr3);
extern uint64_t x86_64_resume_user(const uint64_t *frame, uint64_t process_cr3);
extern __attribute__((noreturn)) void x86_64_user_exit_to_kernel(uint64_t status);

struct direct_memory { uint64_t offset; };

/* Serial logs are intentionally small and allocation-free at early boot. */
static void log_pid(uint32_t pid, const char *event) {
    char number[12];
    unsigned i = 0;
    do {
        number[i++] = (char)('0' + pid % 10u);
        pid /= 10u;
    } while (pid != 0 && i < sizeof(number) - 1);
    char decimal[12];
    unsigned j = 0;
    while (i != 0) decimal[j++] = number[--i];
    decimal[j] = '\0';
    serial_write("IndianOS: PID ");
    serial_write(decimal);
    serial_write(" ");
    serial_write(event);
    serial_write("\n");
}
static struct indianos_user_image *current_user;

/* One active user process at a time. Independent roots, ring-0 stacks and
 * saved IRET contexts survive cooperative yields. No timer IRQ/SMP here. */
#define M009_CONTEXT_WORDS 20u
#define M009_YIELD_STATUS 0x200ull
#define M009_TASK_COUNT 2u
struct m009_task {
    struct indianos_user_image image;
    struct paging_clone_result root;
    struct indianos_process *pcb;
    uint64_t frame[M009_CONTEXT_WORDS] __attribute__((aligned(16)));
    bool saved;
    bool attached;
    uint32_t timer_preemptions;
};
static struct m009_task m009_tasks[M009_TASK_COUNT];
static struct m009_task *m009_active;

/* Every saved context is built from the CPU's own 15 GPR + user IRET
 * frame, not an untrusted user pointer. Timer IRQs require user IF=1;
 * cooperative syscalls require user IF=0. IOPL must always be zero. */
static bool m009_capture_frame(const uint64_t *regs, bool via_timer) {
    if (m009_active == NULL || m009_active->pcb == NULL || regs == NULL ||
        m009_active->pcb->state != INDIANOS_PROCESS_RUNNING ||
        regs[16] != 0x2bull || regs[19] != 0x33ull ||
        (regs[17] & 0x3000ull) != 0 ||
        ((regs[17] & 0x200ull) != 0) != via_timer) return false;
    for (unsigned i = 0; i < M009_CONTEXT_WORDS; ++i)
        m009_active->frame[i] = regs[i];
    if (!via_timer)
        m009_active->frame[14] = 0; /* only SYS_YIELD changes RAX to success */
    m009_active->saved = true;
    return true;
}

/* ACK the PIC even if unexpected. Only user-mode IRQ frames from this
 * bounded active task can cause a context switch. Hardware IRQs are not
 * enabled by the cooperative path or any ordinary IndianOS boot. */
void x86_64_scheduler_timer_dispatch(const uint64_t *regs) {
    x86_64_timer_ack_irq0();
#ifdef INDIANOS_TEST_PREEMPT
    if (m009_capture_frame(regs, true)) {
        if (m009_active->timer_preemptions != UINT32_MAX)
            ++m009_active->timer_preemptions;
        log_pid(m009_active->pcb->pid, "TIMER_PREEMPTED");
        x86_64_user_exit_to_kernel(M009_YIELD_STATUS);
    }
#else
    (void)regs;
#endif
}


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
    if (request.number == INDIANOS_SYS_YIELD) {
        if (!m009_capture_frame(regs, false)) return UINT64_MAX;
        x86_64_user_exit_to_kernel(M009_YIELD_STATUS);
    }
    if (request.number == INDIANOS_SYS_ABI_QUERY)
        return INDIANOS_SYSCALL_ABI;
    if (request.number == INDIANOS_SYS_EXIT)
        x86_64_user_exit_to_kernel(request.arg0);
    if (request.number == INDIANOS_SYS_WRITE) {
#ifdef INDIANOS_TEST_PREEMPT
        static bool reported_user_interrupt_flag;
        if (!reported_user_interrupt_flag) {
            serial_write((regs[17] & 0x200ull) != 0
                ? "IndianOS: M009 user IF=1 (hardware IRQs permitted)\n"
                : "IndianOS: M009 user IF=0 (hardware IRQs masked)\n");
            reported_user_interrupt_flag = true;
        }
#endif
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

bool x86_64_user_process_run(struct indianos_process *process,
                             const uint8_t *elf, size_t size) {
    if (process == NULL || process->pid == 0 ||
        process->state != INDIANOS_PROCESS_NEW || current_user != NULL) {
        return false;
    }
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
    if (!indianos_process_ready(process)) {
        (void)indianos_user_image_destroy(&staged);
        return false;
    }
    log_pid(process->pid, "READY");

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
        if (safe && !indianos_process_start(process, clone.root)) {
            serial_write("IndianOS: ring-3 failed: PCB state transition\n");
            safe = false;
        }
        if (safe) {
            log_pid(process->pid, "RUNNING");
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
            const bool completed = last_user_fault_vector == 0
                ? indianos_process_exit(process, status)
                : indianos_process_fault(process, status, last_user_fault_vector,
                                        last_user_fault_error, last_user_fault_address);
            if (!completed) {
                serial_write("IndianOS: FATAL PCB terminal state rejected\n");
                return false;
            }
            log_pid(process->pid, last_user_fault_vector ? "FAULTED" : "EXITED");
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
    if (!indianos_process_reap(process)) {
        serial_write("IndianOS: ring-3 failed: PCB reap transition\n");
        return false;
    }
    log_pid(process->pid, "REAPED");
    return success;
}

/* Historical QEMU smoke-test API remains a one-task process table. */
bool x86_64_user_demo_run(const uint8_t *elf, size_t size) {
    struct indianos_process_table table = {0};
    if (!indianos_process_table_init(&table)) return false;
    struct indianos_process *task = indianos_process_create(&table);
    return task != NULL && x86_64_user_process_run(task, elf, size);
}

/* M009 cooperative context runner.
 * Two ELF mappings and cloned supervisor roots coexist. Each syscall-based
 * yield saves the complete 64-bit user IRET context and restores kernel CR3,
 * then the next READY task is selected fairly. Never run in normal boots. */
static bool m009_prepare_task(struct m009_task *task,
                              struct indianos_process *pcb,
                              const uint8_t *elf, size_t image_size,
                              uint64_t original,
                              const struct vmm_backend *vmm,
                              const struct paging_clone_backend *backend,
                              unsigned int stack_slot) {
    if (task == NULL || pcb == NULL ||
        !x86_64_user_interrupt_stack_select(stack_slot) ||
        !indianos_user_image_load(&task->image, vmm, elf, image_size))
        return false;
    task->pcb = pcb;
    if (!paging_clone_kernel_half(backend, original, &task->root) ||
        !paging_attach_user_half(backend, task->root.root,
                                task->image.tables.root_physical))
        return false;
    task->attached = true;
    uint64_t trap[INDIANOS_TRAP_MAPPING_COUNT] = {0};
    bool safe = x86_64_user_trap_mapping_addresses(trap);
    const uint64_t supervisor_addresses[] = {
        (uint64_t)(uintptr_t)&x86_64_enter_user,
        (uint64_t)(uintptr_t)&x86_64_resume_user,
        (uint64_t)(uintptr_t)&x86_64_user_exit_to_kernel,
        (uint64_t)(uintptr_t)&x86_64_user_interrupt,
        (uint64_t)(uintptr_t)&x86_64_user_syscall_dispatch,
        (uint64_t)(uintptr_t)&m009_tasks[0],
        (uint64_t)(uintptr_t)&m009_active,
    };
    for (unsigned i = 0; i < INDIANOS_TRAP_MAPPING_COUNT; ++i) {
        if (!valid_kernel_access(backend, original, task->root.root, trap[i]))
            safe = false;
    }
    for (unsigned i = 0;
         i < sizeof(supervisor_addresses)/sizeof(supervisor_addresses[0]); ++i) {
        if (!valid_kernel_access(backend, original, task->root.root,
                                 supervisor_addresses[i])) safe = false;
    }
    uint64_t phys = 0, flags = 0;
    if (!paging_lookup(backend, task->root.root, task->image.plan.entry,
                       &phys, &flags) || (flags & 0x7ull) != 0x5ull ||
        (flags & (1ull << 63)) != 0) safe = false;
    if (!paging_lookup(backend, task->root.root,
                       task->image.plan.stack_end - 16, &phys, &flags) ||
        (flags & 0x7ull) != 0x7ull ||
        (flags & (1ull << 63)) == 0) safe = false;
    if (paging_lookup(backend, task->root.root,
                      task->image.plan.guard_begin, &phys, &flags)) safe = false;
    if (!safe || !indianos_process_ready(pcb) ||
        !indianos_process_bind(pcb, task->root.root)) return false;
    log_pid(pcb->pid, "READY");
    return true;
}

static bool m009_saved_frame_valid(const struct m009_task *task) {
    if (task == NULL || !task->saved ||
        task->frame[16] != 0x2bull || task->frame[19] != 0x33ull ||
        (task->frame[17] & 0x3000ull) != 0 ||
        (task->frame[17] & 0x2ull) == 0) return false;
#ifdef INDIANOS_TEST_PREEMPT
    if ((task->frame[17] & 0x200ull) == 0) return false;
#else
    if ((task->frame[17] & 0x200ull) != 0) return false;
#endif
    const uint64_t rip = task->frame[15];
    const uint64_t rsp = task->frame[18];
    struct vmm_mapping mapping = {0};
    return indianos_program_user_range(&task->image.plan, rip, 1, false) &&
           rsp >= task->image.plan.stack_begin &&
           rsp <= task->image.plan.stack_end &&
           vmm_query_page(&task->image.tables, rip & ~(PAGE - 1), &mapping) &&
           mapping.permissions.user && mapping.permissions.executable &&
           !mapping.permissions.writable;
}

static bool m009_discard_task(struct m009_task *task,
                              const struct paging_clone_backend *backend,
                              bool terminal) {
    if (task == NULL || backend == NULL) return false;
    if (task->attached) {
        if (!paging_detach_user_half(backend, task->root.root,
                                     task->image.tables.root_physical))
            return false;
        task->attached = false;
    }
    if (task->root.root != 0 &&
        !paging_clone_release(backend, &task->root)) return false;
    if (task->image.tables.initialized &&
        !indianos_user_image_destroy(&task->image)) return false;
    if (terminal && (task->pcb == NULL ||
                     !indianos_process_reap(task->pcb))) return false;
    if (terminal) log_pid(task->pcb->pid, "REAPED");
    return true;
}

bool x86_64_m009_schedule_run(const uint8_t *first_elf, size_t first_bytes,
                              const uint8_t *second_elf, size_t second_bytes) {
    if (first_elf == NULL || second_elf == NULL || current_user != NULL ||
        m009_active != NULL) return false;
    struct paging_cpu_state cpu = {0};
    uint64_t offset = 0;
    if (!x86_64_paging_detect(&cpu) || !paging_preflight(&cpu) ||
        cpu.pcid_enabled || !boot_hhdm_offset(&offset) || offset == 0)
        return false;
    struct direct_memory physical = {.offset = offset};
    const struct vmm_backend vmm = {
        .allocate = frame_allocate, .release = frame_release,
        .access = frame_access, .context = &physical,
    };
    const struct paging_clone_backend backend = {
        .allocate = frame_allocate, .release = frame_release,
        .access = table_access, .context = &physical,
    };
    struct indianos_process_table table = {0};
    struct indianos_scheduler scheduler = {0};
    if (!indianos_process_table_init(&table)) return false;
    struct indianos_process *first = indianos_process_create(&table);
    struct indianos_process *second = indianos_process_create(&table);
    if (first == NULL || second == NULL ||
        first->pid != 1 || second->pid != 2)
        return false;
    const uint64_t starting_free = pmm_free_frames();
    for (unsigned i = 0; i < M009_TASK_COUNT; ++i)
        m009_tasks[i] = (struct m009_task){0};

    bool prepared = m009_prepare_task(&m009_tasks[0], first,
                                       first_elf, first_bytes,
                                       cpu.cr3 & PHYS_MASK,
                                       &vmm, &backend, 0);
    if (prepared) {
        prepared = m009_prepare_task(&m009_tasks[1], second,
                                      second_elf, second_bytes,
                                      cpu.cr3 & PHYS_MASK,
                                      &vmm, &backend, 1);
    }
    if (!prepared || !indianos_scheduler_init(&scheduler, &table)) {
        serial_write("IndianOS: M009 FATAL cannot prepare two user contexts\n");
        (void)m009_discard_task(&m009_tasks[1], &backend, false);
        (void)m009_discard_task(&m009_tasks[0], &backend, false);
        return false;
    }
    serial_write("IndianOS: M009 two simultaneous user address spaces ready\n");
#ifdef INDIANOS_TEST_PREEMPT
    if (!x86_64_timer_start_100hz()) {
        serial_write("IndianOS: M009 FATAL cannot configure PIC/PIT timer\n");
        return false;
    }
    serial_write("IndianOS: M009 hardware PIT IRQ0 enabled at 100 Hz\n");
#endif
    bool ok = true;
    while (ok && !indianos_scheduler_all_terminal(&scheduler)) {
        struct indianos_process *pcb = indianos_scheduler_next(&scheduler);
        if (pcb == NULL || pcb->pid < 1 || pcb->pid > M009_TASK_COUNT) {
            ok = false;
            break;
        }
        const unsigned slot = pcb->pid - 1u;
        struct m009_task *task = &m009_tasks[slot];
        if (task->pcb != pcb ||
            !x86_64_user_interrupt_stack_select(slot)) {
            ok = false;
            break;
        }
        if (task->saved) {
            if (!m009_saved_frame_valid(task) ||
                !indianos_process_resume(pcb)) {
                ok = false;
                break;
            }
        } else if (!indianos_process_start(pcb, task->root.root)) {
            ok = false;
            break;
        }
        log_pid(pcb->pid, "DISPATCHED");
        last_user_fault_vector = 0;
        last_user_fault_error = 0;
        last_user_fault_address = 0;
        current_user = &task->image;
        m009_active = task;
        uint64_t status = task->saved
            ? x86_64_resume_user(task->frame, task->root.root)
            : x86_64_enter_user(task->image.plan.entry,
                                 task->image.plan.stack_end - 16, task->root.root);
        m009_active = NULL;
        current_user = NULL;
        uint64_t active_cr3 = 0;
        __asm__ volatile ("mov %%cr3, %0" : "=r"(active_cr3));
        if ((active_cr3 & PHYS_MASK) != (cpu.cr3 & PHYS_MASK)) {
            serial_write("IndianOS: M009 FATAL CR3 was not restored\n");
            return false; /* Never free frames under an active user root. */
        }
        if (status == M009_YIELD_STATUS && task->saved &&
            last_user_fault_vector == 0) {
            if (!indianos_scheduler_yield(&scheduler, pcb)) {
                ok = false;
                break;
            }
            log_pid(pcb->pid, "YIELDED");
            indianos_scheduler_account_tick(&scheduler);
        } else {
            if (status == M009_YIELD_STATUS) {
                ok = false;
                break;
            }
            const bool finished = last_user_fault_vector != 0
                ? indianos_process_fault(pcb, status,
                                        last_user_fault_vector,
                                        last_user_fault_error,
                                        last_user_fault_address)
                : indianos_process_exit(pcb, status);
            if (!finished) {
                ok = false;
                break;
            }
            log_pid(pcb->pid, last_user_fault_vector ? "FAULTED" : "EXITED");
            if (!m009_discard_task(task, &backend, true)) {
                ok = false;
                break;
            }
        }
    }
    if (ok && (!indianos_scheduler_all_terminal(&scheduler) ||
               !indianos_process_success(first) ||
               !indianos_process_success(second) ||
               pmm_free_frames() != starting_free))
        ok = false;
#ifdef INDIANOS_TEST_PREEMPT
    if (x86_64_timer_irq_count() == 0)
        serial_write("IndianOS: M009 PIC/PIT delivered zero timer IRQs\n");
    else
        serial_write("IndianOS: M009 PIC/PIT hardware timer interrupts observed\n");
    if (scheduler.yield_count < 2 || scheduler.tick_count < 2 ||
        m009_tasks[0].timer_preemptions == 0 ||
        m009_tasks[1].timer_preemptions == 0 ||
        x86_64_timer_irq_count() < 2)
        ok = false;
    if (ok) serial_write("IndianOS: M009 timer-preemptive round-robin PASSED\n");
#else
    if (scheduler.dispatch_count != 6 || scheduler.yield_count != 4 ||
        scheduler.tick_count != 4) ok = false;
    if (ok) serial_write("IndianOS: M009 round-robin cooperative switching PASSED\n");
#endif
    if (!ok) serial_write("IndianOS: M009 FATAL scheduler or memory ownership verification\n");
    return ok;
}
