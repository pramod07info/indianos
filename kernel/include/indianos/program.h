#ifndef INDIANOS_PROGRAM_H
#define INDIANOS_PROGRAM_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

/* Planning only: segments are NOT mapped and entry points are NOT run. */
#define INDIANOS_USER_MIN 0x0000000000400000ull
#define INDIANOS_USER_TOP 0x00007ffff0000000ull
#define INDIANOS_USER_STACK_BYTES (64ull * 1024ull)
#define INDIANOS_USER_PAGE_BYTES 4096ull
#define INDIANOS_MAX_PROGRAM_SEGMENTS 8u
#define INDIANOS_MAX_ELF_BYTES (32u * 1024u * 1024u)

struct indianos_program_segment {
    uint64_t virtual_address;
    uint64_t file_offset;
    uint64_t file_size;
    uint64_t memory_size;
    uint64_t page_begin;
    uint64_t page_end; /* exclusive */
    bool writable;
    bool executable;
};

struct indianos_program_plan {
    uint64_t entry;
    uint64_t stack_begin;
    uint64_t stack_end;
    uint64_t guard_begin;
    uint32_t segment_count;
    struct indianos_program_segment segments[INDIANOS_MAX_PROGRAM_SEGMENTS];
};

/* Strict, static ET_EXEC, little-endian ELF64/x86-64 only, no PT_INTERP.
 * Fails closed on malformed ranges and page-granularity overlap. */
bool indianos_program_plan_elf64(const uint8_t *elf, size_t elf_size,
                                struct indianos_program_plan *plan);
/* Proves only that bytes fall inside one planned mapped segment or stack.
 * NOT a substitute for VMM permissions and copy_from_user. */
bool indianos_program_user_range(const struct indianos_program_plan *plan,
                                 uint64_t address, uint64_t length, bool writable);
bool indianos_program_self_test(void);

#endif
