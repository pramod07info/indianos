#ifndef INDIANOS_TIMER_H
#define INDIANOS_TIMER_H

#include <stdbool.h>
#include <stdint.h>

/* QEMU-only legacy 8259 PIC / PIT interrupt experiment. IRQ0->IDT 0x20.
 * No APIC/SMP support. Timer interrupts may run only after process CR3
 * mapping and TSS.RSP0 preflight; user CS is validated in dispatch. */
bool x86_64_timer_start_100hz(void);
void x86_64_timer_ack_irq0(void);
uint64_t x86_64_timer_irq_count(void);
uint8_t x86_64_timer_pic_pending(void);
uint8_t x86_64_timer_pic_mask(void);

#endif
