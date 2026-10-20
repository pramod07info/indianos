#include <indianos/boot.h>
#include <indianos/serial.h>
#include <indianos/interrupts.h>
#include <indianos/pmm.h>
#include <indianos/vmm.h>
#include <indianos/paging.h>
#include <indianos/program.h>
#include <indianos/syscall_abi.h>
#include <indianos/user_image.h>

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

    /* M005 foundation only: parse a built-in synthetic user ELF. No
     * page mappings, ring-3 switch or SYSCALL entry are performed. */
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
#ifdef INDIANOS_TEST_PAGEFAULT
    serial_write("IndianOS: triggering intentional page fault\n");
    __asm__ volatile ("movabs $0x00007fff00000000, %%rax; movq (%%rax), %%rax"
                      : : : "rax", "memory");
#endif
    halt_forever();
}
