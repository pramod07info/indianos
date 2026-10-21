#ifndef INDIANOS_INTERRUPTS_H
#define INDIANOS_INTERRUPTS_H

#include <stdbool.h>
#include <stdint.h>
#include <stddef.h>

/* Installs a minimal kernel-only GDT and IDT; external interrupts stay off. */
bool x86_64_exceptions_init(void);
/* Only enable for the QEMU-only user-mode experiment after IDT is ready. */
bool x86_64_user_interrupt_stack_enable(void);
/* M009: two distinct ring-0 syscall stacks, selected before each process
 * resumes in ring 3. Interrupts stay masked; no hardware preemption yet. */
#define INDIANOS_USER_KERNEL_STACK_COUNT 2u
bool x86_64_user_interrupt_stack_select(unsigned int slot);

/* Addresses of CPU descriptor/trap state that MUST remain supervisor mapped
 * in a switched CR3. Used by QEMU-only ring-3 preflight. */
#define INDIANOS_TRAP_MAPPING_COUNT 5u
bool x86_64_user_trap_mapping_addresses(uint64_t result[INDIANOS_TRAP_MAPPING_COUNT]);

#endif
