#include <indianos/interrupts.h>
#include <indianos/serial.h>
#include <stddef.h>
#include <stdint.h>

#define INTERRUPT_GATE 0x8eu
#define KERNEL_CODE_SELECTOR 0x08u
#define TSS_SELECTOR 0x18u
#define DOUBLE_FAULT_IST_INDEX 1u
#define USER_SYSCALL_VECTOR 0x80u
#define USER_INTERRUPT_GATE 0xeeu

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

struct __attribute__((packed)) tss64 {
    uint32_t reserved0;
    uint64_t rsp0, rsp1, rsp2;
    uint64_t reserved1;
    uint64_t ist1, ist2, ist3, ist4, ist5, ist6, ist7;
    uint64_t reserved2;
    uint16_t reserved3;
    uint16_t iomap_base;
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
extern void x86_64_user_interrupt(void);
static uint8_t user_kernel_stack[32768] __attribute__((aligned(16)));

/* Four contiguous 64-bit descriptor slots plus null; the TSS occupies 2. */
static uint64_t gdt[7] __attribute__((aligned(16))) = {
    0x0000000000000000ull,
    0x00af9a000000ffffull, /* Ring-0, 64-bit executable segment. */
    0x00cf92000000ffffull, /* Ring-0 data segment. */
    0, 0,                /* 16-byte TSS descriptor, filled at runtime. */
    0x00affa000000ffffull, /* Ring-3, 64-bit executable segment, selector 0x2b. */
    0x00cff2000000ffffull, /* Ring-3 data, selector 0x33. */
};
static struct idt_entry idt[256] __attribute__((aligned(16)));
static struct tss64 tss __attribute__((aligned(16)));
/* A dedicated stack for #DF, separate from a possibly corrupted RSP. */
static uint8_t double_fault_stack[16384] __attribute__((aligned(16)));

static void set_gate(uint8_t vector, void (*handler)(void), uint8_t ist, uint8_t attr) {
    uint64_t address = (uint64_t)(uintptr_t)handler;
    idt[vector] = (struct idt_entry) {
        .offset_low = (uint16_t)address,
        .selector = KERNEL_CODE_SELECTOR,
        .ist = ist,
        .attributes = attr,
        .offset_mid = (uint16_t)(address >> 16),
        .offset_high = (uint32_t)(address >> 32),
        .reserved = 0,
    };
}

static void install_tss_descriptor(void) {
    const uint64_t base = (uint64_t)(uintptr_t)&tss;
    const uint64_t limit = sizeof(tss) - 1ull;
    gdt[3] = (limit & 0xffffull) |
             ((base & 0xffffffull) << 16) |
             (0x89ull << 40) |
             (((limit >> 16) & 0xfull) << 48) |
             (((base >> 24) & 0xffull) << 56);
    gdt[4] = base >> 32;
}

bool x86_64_exceptions_init(void) {
    tss.ist1 = (uint64_t)(uintptr_t)(double_fault_stack + sizeof(double_fault_stack));
    tss.iomap_base = (uint16_t)sizeof(tss); /* No I/O permission bitmap. */
    install_tss_descriptor();
    const struct descriptor_ptr gdtr = {
        .limit = sizeof(gdt) - 1,
        .address = (uint64_t)(uintptr_t)gdt,
    };
    x86_64_load_gdt(&gdtr);
    /* LTR sets the processor's task register; required before using IST. */
    const uint16_t selector = TSS_SELECTOR;
    __asm__ volatile ("ltr %0" : : "rm"(selector) : "memory");
    for (unsigned int i = 0; i < 256; ++i) {
        set_gate((uint8_t)i, x86_64_unexpected_interrupt, 0, INTERRUPT_GATE);
    }
    for (unsigned int i = 0; i < 32; ++i) {
        const uint8_t ist = i == 8 ? DOUBLE_FAULT_IST_INDEX : 0;
        set_gate((uint8_t)i, x86_64_exception_stubs[i], ist, INTERRUPT_GATE);
    }
    /* DPL 3 allows software INT 0x80, but no other user-callable gate. */
    set_gate(USER_SYSCALL_VECTOR, x86_64_user_interrupt, 0, USER_INTERRUPT_GATE);
    const struct descriptor_ptr idtr = {
        .limit = sizeof(idt) - 1,
        .address = (uint64_t)(uintptr_t)idt,
    };
    __asm__ volatile ("lidt %0" : : "m"(idtr) : "memory");
    /* Hardware interrupts remain off. RSP0 is enabled only for opt-in test. */
    return true;
}

bool x86_64_user_interrupt_stack_enable(void) {
    if (tss.ist1 == 0) return false;
    tss.rsp0 = (uint64_t)(uintptr_t)(user_kernel_stack + sizeof(user_kernel_stack));
    return tss.rsp0 != 0;
}

bool x86_64_user_trap_mapping_addresses(
        uint64_t result[INDIANOS_TRAP_MAPPING_COUNT]) {
    if (result == NULL || tss.ist1 == 0 || tss.rsp0 == 0) return false;
    result[0] = (uint64_t)(uintptr_t)&gdt[0];
    result[1] = (uint64_t)(uintptr_t)&idt[0];
    result[2] = (uint64_t)(uintptr_t)&tss;
    result[3] = tss.rsp0 - 8; /* final mapped byte on kernel syscall stack */
    result[4] = tss.ist1 - 8; /* emergency double-fault stack */
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
    if (context->vector == 14) {
        uint64_t cr2 = 0;
        __asm__ volatile ("mov %%cr2, %0" : "=r"(cr2));
        serial_write("IndianOS: page fault cr2=");
        serial_write_hex(cr2);
        serial_write(" present=");
        serial_write((context->error_code & 1ull) ? "1" : "0");
        serial_write(" write=");
        serial_write((context->error_code & 2ull) ? "1" : "0");
        serial_write(" user=");
        serial_write((context->error_code & 4ull) ? "1" : "0");
        serial_write(" reserved=");
        serial_write((context->error_code & 8ull) ? "1" : "0");
        serial_write(" instruction=");
        serial_write((context->error_code & 16ull) ? "1" : "0");
        serial_write("\n");
    }
    for (;;) {
        __asm__ volatile ("cli; hlt" : : : "memory");
    }
}
