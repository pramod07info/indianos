#include <indianos/serial.h>
#include <stdint.h>

/* QEMU's 16550-compatible UART, using COM1 polling before any interrupts. */
#define COM1 0x3f8u

static inline void outb(uint16_t port, uint8_t value) {
    __asm__ volatile ("outb %0, %1" : : "a"(value), "Nd"(port));
}

static inline uint8_t inb(uint16_t port) {
    uint8_t value;
    __asm__ volatile ("inb %1, %0" : "=a"(value) : "Nd"(port));
    return value;
}

void serial_init(void) {
    outb(COM1 + 1, 0x00); /* Disable interrupts. */
    outb(COM1 + 3, 0x80); /* Divisor latch. */
    outb(COM1,     0x03); /* 38400 baud. */
    outb(COM1 + 1, 0x00);
    outb(COM1 + 3, 0x03); /* 8 data bits, no parity, one stop bit. */
    outb(COM1 + 2, 0xc7); /* Enable and clear FIFO. */
    outb(COM1 + 4, 0x0b); /* DTR, RTS, OUT2. */
}

static void serial_putc(char c) {
    /* A bounded poll prevents a missing UART from hanging kernel startup. */
    for (unsigned int poll = 0; poll < 100000u; ++poll) {
        if ((inb(COM1 + 5) & 0x20u) != 0) {
            outb(COM1, (uint8_t)c);
            return;
        }
    }
}

void serial_write(const char *message) {
    if (message == 0) {
        return;
    }
    while (*message != '\0') {
        if (*message == '\n') {
            serial_putc('\r');
        }
        serial_putc(*message++);
    }
}
