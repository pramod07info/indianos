#include <indianos/boot.h>
#include <indianos/serial.h>
#include <indianos/interrupts.h>
#include <indianos/pmm.h>
#include <indianos/vmm.h>
#include <indianos/paging.h>
#include <indianos/program.h>
#include <indianos/syscall_abi.h>
#include <indianos/user_image.h>
#include <indianos/user_mode.h>
#include <indianos/process.h>

__attribute__((noreturn)) static void halt_forever(void) {
    for (;;) {
        __asm__ volatile ("cli; hlt" : : : "memory");
    }
}

/* Limine enters in 64-bit long mode with a stack; no C runtime is assumed. */
__attribute__((noreturn)) void kmain(void) {
    __asm__ volatile ("cli" : : : "memory");
    serial_init();
    serial_write("IndianOS: kernel entered\n");

    if (!boot_validate()) {
        serial_write("IndianOS: FATAL boot information invalid\n");
        halt_forever();
    }
    serial_write("IndianOS: boot information validated\n");

    /* Drawing is optional; a serial-only machine can still boot M001. */
    boot_draw_status();
    serial_write("IndianOS: milestone 001 complete\n");

    if (!x86_64_exceptions_init()) {
        serial_write("IndianOS: FATAL exception table installation failed\n");
        halt_forever();
    }
    serial_write("IndianOS: exception handlers ready\n");

    if (!pmm_initialize(boot_memory_region_count(), boot_memory_region_at, 0)) {
        serial_write("IndianOS: FATAL physical memory initialization failed\n");
        halt_forever();
    }
    if (!pmm_self_test()) {
        serial_write("IndianOS: FATAL physical memory self-test failed\n");
        halt_forever();
    }
    serial_write("IndianOS: physical memory manager ready\n");

#ifdef INDIANOS_TEST_UD2
    serial_write("IndianOS: triggering intentional UD2\n");
    __asm__ volatile ("ud2");
#endif
    serial_write("IndianOS: milestone 002 complete\n");

    /* Builds and validates an UNUSED address-space tree, not active CR3. */
    if (!vmm_boot_self_test()) {
        serial_write("IndianOS: FATAL virtual memory self-test failed\n");
        halt_forever();
    }
    serial_write("IndianOS: detached virtual memory manager ready\n");
    serial_write("IndianOS: milestone 003 complete\n");

    struct paging_cpu_state paging_state = {0};
    if (!x86_64_paging_detect(&paging_state)) {
        serial_write("IndianOS: FATAL paging capability discovery failed\n");
        halt_forever();
    }
    serial_write("IndianOS: paging capabilities inspected\n");
    if (paging_preflight(&paging_state)) {
        serial_write("IndianOS: shadow paging preflight passed\n");
    } else {
        /* Basic kernel remains usable; no unsafe change to live CR3. */
        serial_write("IndianOS: shadow paging switch not permitted\n");
    }
#ifdef INDIANOS_TEST_CR3_SWITCH
    if (!x86_64_paging_shadow_switch_test()) {
        serial_write("IndianOS: FATAL shadow CR3 switch failed\n");
        halt_forever();
    }
    serial_write("IndianOS: shadow CR3 switched and restored\n");
#endif
    serial_write("IndianOS: milestone 004 complete\n");

    /* Validate a synthetic ELF first; below, a real /bin/init is temporarily
     * mapped into DETACHED page tables. No ring-3 entry, live CR3 switch
     * or SYSCALL dispatch is performed in a normal boot. */
    if (!indianos_program_self_test() || !indianos_syscall_self_test()) {
        serial_write("IndianOS: FATAL user image contract self-test failed\n");
        halt_forever();
    }
    serial_write("IndianOS: ELF64 user-image validation ready\n");
    serial_write("IndianOS: syscall ABI metadata validation ready\n");
    serial_write("IndianOS: milestone 005 foundation complete\n");

    const uint8_t *init_elf = 0;
    size_t init_elf_size = 0;
    if (!boot_initramfs_file("/bin/init", &init_elf, &init_elf_size)) {
        serial_write("IndianOS: FATAL initramfs /bin/init is unavailable or invalid\n");
        halt_forever();
    }
    serial_write("IndianOS: read-only initramfs /bin/init validated\n");
    if (!user_image_boot_stage_test(init_elf, init_elf_size)) {
        serial_write("IndianOS: FATAL detached user-image staging failed\n");
        halt_forever();
    }
    serial_write("IndianOS: initramfs /bin/init staged and released safely\n");
#ifdef INDIANOS_TEST_SCHEDULER
    /* Different ELF images share a virtual entry address, but never physical
     * user frames or cloned CR3 roots. Four cooperative yields produce six
     * round-robin dispatches; every resource is reaped before success. */
    const uint8_t *sched_worker_elf = 0;
    size_t sched_worker_bytes = 0;
    if (!boot_initramfs_file("/bin/worker",
                            &sched_worker_elf, &sched_worker_bytes)) {
        serial_write("IndianOS: FATAL M009 worker executable not found\n");
        halt_forever();
    }
    serial_write("IndianOS: M009 cooperative scheduler experiment begin\n");
    if (!x86_64_m009_schedule_run(init_elf, init_elf_size,
                                   sched_worker_elf, sched_worker_bytes)) {
        serial_write("IndianOS: FATAL M009 cooperative context test failed\n");
        halt_forever();
    }
    serial_write("IndianOS: M009 scheduler experiment complete\n");
#endif
#ifdef INDIANOS_TEST_PROCESSES
    /* Two separate ELF images: both undergo fresh frame allocations and
     * independent CR3 clones; the second starts only AFTER the first is
     * completely reaped. No scheduler, preemption or parallel execution. */
    const uint8_t *worker_elf = 0;
    size_t worker_elf_size = 0;
    if (!boot_initramfs_file("/bin/worker", &worker_elf, &worker_elf_size)) {
        serial_write("IndianOS: FATAL second user executable missing\n");
        halt_forever();
    }
    struct indianos_process_table processes = {0};
    if (!indianos_process_table_init(&processes)) {
        serial_write("IndianOS: FATAL process table initialization failed\n");
        halt_forever();
    }
    struct indianos_process *first = indianos_process_create(&processes);
    struct indianos_process *second = indianos_process_create(&processes);
    if (first == 0 || second == 0 || first->pid != 1 || second->pid != 2) {
        serial_write("IndianOS: FATAL process ID assignment failed\n");
        halt_forever();
    }
    serial_write("IndianOS: M008 sequential process launch begin\n");
    if (!x86_64_user_process_run(first, init_elf, init_elf_size) ||
        !indianos_process_success(first)) {
        serial_write("IndianOS: FATAL first process did not exit and reap\n");
        halt_forever();
    }
    if (!x86_64_user_process_run(second, worker_elf, worker_elf_size) ||
        !indianos_process_success(second)) {
        serial_write("IndianOS: FATAL second process did not exit and reap\n");
        halt_forever();
    }
    serial_write("IndianOS: M008 two independent processes exited and reaped\n");
#endif
#if defined(INDIANOS_TEST_RING3) || \
    defined(INDIANOS_TEST_USERFAULT_GUARD) || \
    defined(INDIANOS_TEST_USERFAULT_SUPERVISOR)
    if (!x86_64_user_demo_run(init_elf, init_elf_size)) {
        serial_write("IndianOS: FATAL ring-3 user program experiment failed\n");
        halt_forever();
    }
#if defined(INDIANOS_TEST_USERFAULT_GUARD) || \
    defined(INDIANOS_TEST_USERFAULT_SUPERVISOR)
    serial_write("IndianOS: user fault isolation experiment complete\n");
#else
    serial_write("IndianOS: ring-3 user program experiment complete\n");
#endif
#endif
#ifdef INDIANOS_TEST_PAGEFAULT
    serial_write("IndianOS: triggering intentional page fault\n");
    __asm__ volatile ("movabs $0x00007fff00000000, %%rax; movq (%%rax), %%rax"
                      : : : "rax", "memory");
#endif
    halt_forever();
}
