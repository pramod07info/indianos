#include <indianos/boot.h>
#include <indianos/serial.h>

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
    halt_forever();
}
