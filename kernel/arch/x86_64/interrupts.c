#include <indianos/interrupts.h>
#include <indianos/serial.h>
#include <stddef.h>
#include <stdint.h>

#define INTERRUPT_GATE 0x8eu
#define KERNEL_CODE_SELECTOR 0x08u

struct __attribute__((packed)) descriptor_ptr {
    uint16_t limit;
    uint64_t address;
};

struct __attribute__((packed)) idt_entry {
    uint16_t offset_low;
    uint16_t selector;
    uint8_t ist;
    uint8_t attributes;
    uint16_t offset_mid;
    uint32_t offset_high;
    uint32_t reserved;
};

struct exception_context {
    uint64_t vector;
    uint64_t error_code;
    uint64_t rip;
    uint64_t cs;
    uint64_t rflags;
};

extern void x86_64_load_gdt(const struct descriptor_ptr *descriptor);
extern void (*const x86_64_exception_stubs[32])(void);
extern void x86_64_unexpected_interrupt(void);

static const uint64_t gdt[3] __attribute__((aligned(16))) = {
    0x0000000000000000ull,
    0x00af9a000000ffffull, /* Ring-0, 64-bit executable segment. */
    0x00cf92000000ffffull, /* Ring-0, data segment. */
};
static struct idt_entry idt[256] __attribute__((aligned(16)));

static void set_gate(uint8_t vector, void (*handler)(void)) {
    uint64_t address = (uint64_t)(uintptr_t)handler;
    idt[vector] = (struct idt_entry) {
        .offset_low = (uint16_t)address,
        .selector = KERNEL_CODE_SELECTOR,
        .ist = 0,
        .attributes = INTERRUPT_GATE,
        .offset_mid = (uint16_t)(address >> 16),
        .offset_high = (uint32_t)(address >> 32),
        .reserved = 0,
    };
}

bool x86_64_exceptions_init(void) {
    const struct descriptor_ptr gdtr = {
        .limit = sizeof(gdt) - 1,
        .address = (uint64_t)(uintptr_t)gdt,
    };
    x86_64_load_gdt(&gdtr);
    for (unsigned int i = 0; i < 256; ++i) {
        set_gate((uint8_t)i, x86_64_unexpected_interrupt);
    }
    for (unsigned int i = 0; i < 32; ++i) {
        set_gate((uint8_t)i, x86_64_exception_stubs[i]);
    }
    const struct descriptor_ptr idtr = {
        .limit = sizeof(idt) - 1,
        .address = (uint64_t)(uintptr_t)idt,
    };
    __asm__ volatile ("lidt %0" : : "m"(idtr) : "memory");
    /* No PIC/APIC setup or interrupt enabling in M002. */
    return true;
}

static void serial_write_hex(uint64_t value) {
    static const char hex[] = "0123456789abcdef";
    char buffer[19];
    buffer[0] = '0';
    buffer[1] = 'x';
    for (unsigned int i = 0; i < 16; ++i) {
        buffer[2 + i] = hex[(value >> (60u - 4u * i)) & 15u];
    }
    buffer[18] = '\0';
    serial_write(buffer);
}

__attribute__((noreturn)) void x86_64_exception_dispatch(
        const struct exception_context *context) {
    serial_write("IndianOS: FATAL CPU exception vector=");
    serial_write_hex(context->vector);
    serial_write(" error=");
    serial_write_hex(context->error_code);
    serial_write(" rip=");
    serial_write_hex(context->rip);
    serial_write("\n");
    for (;;) {
        __asm__ volatile ("cli; hlt" : : : "memory");
    }
}
