#include <indianos/timer.h>
#include <stdbool.h>
#include <stdint.h>

#define PIC1_CMD  0x20u
#define PIC1_DATA 0x21u
#define PIC2_CMD  0xa0u
#define PIC2_DATA 0xa1u
#define PIT_CMD   0x43u
#define PIT_DATA  0x40u

static uint64_t irq_count;
static bool configured;

static inline void outb(uint16_t port, uint8_t byte) {
    __asm__ volatile ("outb %0,%1" : : "a"(byte), "Nd"(port));
}
static inline void io_wait(void) { outb(0x80u, 0); }
static inline uint8_t inb(uint16_t port) {
    uint8_t byte;
    __asm__ volatile ("inb %1,%0" : "=a"(byte) : "Nd"(port));
    return byte;
}

/* The UEFI boot environment may leave the LAPIC enabled but its LINT0
 * virtual-wire input masked. For this QEMU-only legacy 8259 experiment,
 * disable xAPIC before expecting PIC->INTR delivery. A firmware-enabled
 * x2APIC configuration is not changed: fail closed instead. */
static bool select_legacy_pic_delivery(void) {
    const uint32_t msr = 0x1bu; /* IA32_APIC_BASE */
    uint32_t lo = 0, hi = 0;
    __asm__ volatile ("rdmsr" : "=a"(lo), "=d"(hi) : "c"(msr));
    if ((lo & (1u << 10)) != 0) return false; /* x2APIC unsupported */
    if ((lo & (1u << 11)) != 0) {
        lo &= ~(1u << 11);
        __asm__ volatile ("wrmsr" : : "a"(lo), "d"(hi), "c"(msr) : "memory");
    }
    return true;
}

bool x86_64_timer_start_100hz(void) {
    if (configured || !select_legacy_pic_delivery()) return false;
    /* PICs are remapped because exception vectors 0x00..0x1f cannot host
     * hardware interrupts. Mask all except IRQ0 on master, all on slave. */
    outb(PIC1_CMD, 0x11); io_wait();
    outb(PIC2_CMD, 0x11); io_wait();
    outb(PIC1_DATA, 0x20); io_wait();
    outb(PIC2_DATA, 0x28); io_wait();
    outb(PIC1_DATA, 0x04); io_wait();
    outb(PIC2_DATA, 0x02); io_wait();
    outb(PIC1_DATA, 0x01); io_wait();
    outb(PIC2_DATA, 0x01); io_wait();
    outb(PIC1_DATA, 0xfe); /* only IRQ0 (PIT) unmasked */
    outb(PIC2_DATA, 0xff);
    /* QEMU/PC firmware can route 8259 INTR through the APIC IMCR.
     * Explicitly select PIC-to-CPU virtual-wire routing for this 8259 test.
     * This opt-in experimental routine is never called in normal boots. */
    outb(0x22u, 0x70u); io_wait();
    outb(0x23u, 0x00u); io_wait();
    /* PIT 1193182 / 11932 ~100 Hz, channel 0, square wave. */
    const uint16_t divisor = 11932;
    outb(PIT_CMD, 0x36);
    outb(PIT_DATA, (uint8_t)(divisor & 0xff));
    outb(PIT_DATA, (uint8_t)(divisor >> 8));
    irq_count = 0;
    configured = true;
    return true;
}

void x86_64_timer_ack_irq0(void) {
    if (!configured) return;
    if (irq_count != UINT64_MAX) ++irq_count;
    outb(PIC1_CMD, 0x20); /* End of interrupt for master IRQ0. */
}
uint64_t x86_64_timer_irq_count(void) { return irq_count; }
uint8_t x86_64_timer_pic_pending(void) {
    if (!configured) return 0;
    outb(PIC1_CMD, 0x0au); /* OCW3: return interrupt request register. */
    return inb(PIC1_CMD);
}
uint8_t x86_64_timer_pic_mask(void) {
    return configured ? inb(PIC1_DATA) : 0xffu;
}
