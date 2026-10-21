#ifndef INDIANOS_INTERRUPTS_H
#define INDIANOS_INTERRUPTS_H

#include <stdbool.h>

/* Installs a minimal kernel-only GDT and IDT; external interrupts stay off. */
bool x86_64_exceptions_init(void);
/* Only enable for the QEMU-only user-mode experiment after IDT is ready. */
bool x86_64_user_interrupt_stack_enable(void);

#endif
