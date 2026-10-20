#include <indianos/boot.h>
#include <indianos/serial.h>
#include <indianos/interrupts.h>
#include <indianos/pmm.h>
#include <indianos/vmm.h>

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
    halt_forever();
}
