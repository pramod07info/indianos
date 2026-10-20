#include <indianos/program.h>

#define ELF_EHDR_BYTES 64u
#define ELF_PHDR_BYTES 56u
#define ELF_ET_EXEC 2u
#define ELF_MACHINE_X86_64 62u
#define ELF_PT_LOAD 1u
#define ELF_PT_DYNAMIC 2u
#define ELF_PT_INTERP 3u
#define ELF_PT_TLS 7u
#define ELF_PT_GNU_STACK 0x6474e551u
#define ELF_PF_X 1u
#define ELF_PF_W 2u
#define ELF_PF_R 4u
#define PAGE_MASK (INDIANOS_USER_PAGE_BYTES - 1ull)

static uint16_t rd16(const uint8_t *p) {
    return (uint16_t)((uint16_t)p[0] | (uint16_t)((uint16_t)p[1] << 8));
}
static uint32_t rd32(const uint8_t *p) {
    return (uint32_t)p[0] | ((uint32_t)p[1] << 8) |
           ((uint32_t)p[2] << 16) | ((uint32_t)p[3] << 24);
}
static uint64_t rd64(const uint8_t *p) {
    return (uint64_t)rd32(p) | ((uint64_t)rd32(p + 4) << 32);
}
static bool in_bounds(uint64_t offset, uint64_t length, uint64_t total) {
    return offset <= total && length <= total - offset;
}
static bool disjoint(uint64_t a0, uint64_t a1, uint64_t b0, uint64_t b1) {
    return a1 <= b0 || b1 <= a0;
}

bool indianos_program_plan_elf64(const uint8_t *elf, size_t elf_size,
                                struct indianos_program_plan *plan) {
    if (elf == NULL || plan == NULL || elf_size < ELF_EHDR_BYTES ||
        elf_size > INDIANOS_MAX_ELF_BYTES) return false;
    if (elf[0] != 0x7f || elf[1] != 'E' || elf[2] != 'L' || elf[3] != 'F' ||
        elf[4] != 2 || elf[5] != 1 || elf[6] != 1 || elf[7] != 0 ||
        rd16(elf + 16) != ELF_ET_EXEC ||
        rd16(elf + 18) != ELF_MACHINE_X86_64 || rd32(elf + 20) != 1 ||
        rd16(elf + 52) != ELF_EHDR_BYTES ||
        rd16(elf + 54) != ELF_PHDR_BYTES) return false;
    for (unsigned int i = 8; i < 16; ++i) {
        if (elf[i] != 0) return false;
    }
    const uint64_t entry = rd64(elf + 24);
    const uint64_t phoff = rd64(elf + 32);
    const uint16_t phnum = rd16(elf + 56);
    if (phnum == 0 || phnum > 32 ||
        !in_bounds(phoff, (uint64_t)phnum * ELF_PHDR_BYTES, elf_size) ||
        entry < INDIANOS_USER_MIN || entry >= INDIANOS_USER_TOP) return false;

    struct indianos_program_plan candidate = {0};
    candidate.entry = entry;
    candidate.stack_end = INDIANOS_USER_TOP;
    candidate.stack_begin = INDIANOS_USER_TOP - INDIANOS_USER_STACK_BYTES;
    candidate.guard_begin = candidate.stack_begin - INDIANOS_USER_PAGE_BYTES;
    bool executable_entry = false;
    for (uint16_t i = 0; i < phnum; ++i) {
        const uint8_t *ph = elf + (size_t)phoff + (size_t)i * ELF_PHDR_BYTES;
        const uint32_t type = rd32(ph);
        const uint32_t flags = rd32(ph + 4);
        if (type == ELF_PT_INTERP || type == ELF_PT_DYNAMIC || type == ELF_PT_TLS ||
            (type == ELF_PT_GNU_STACK && (flags & ELF_PF_X) != 0)) return false;
        if (type != ELF_PT_LOAD) continue;
        if (candidate.segment_count == INDIANOS_MAX_PROGRAM_SEGMENTS ||
            (flags & ~(ELF_PF_R | ELF_PF_W | ELF_PF_X)) != 0 ||
            (flags & ELF_PF_R) == 0 ||
            (flags & (ELF_PF_W | ELF_PF_X)) == (ELF_PF_W | ELF_PF_X)) return false;
        const uint64_t offset = rd64(ph + 8);
        const uint64_t va = rd64(ph + 16);
        const uint64_t file_size = rd64(ph + 32);
        const uint64_t memory_size = rd64(ph + 40);
        const uint64_t align = rd64(ph + 48);
        if (memory_size == 0 || file_size > memory_size ||
            !in_bounds(offset, file_size, elf_size) ||
            va < INDIANOS_USER_MIN || va >= candidate.guard_begin ||
            memory_size > candidate.guard_begin - va ||
            (offset & PAGE_MASK) != (va & PAGE_MASK) ||
            (align > 1 && ((align & (align - 1)) != 0 ||
                           (va & (align - 1)) != (offset & (align - 1))))) return false;
        const uint64_t end = va + memory_size;
        if (end > UINT64_MAX - PAGE_MASK) return false;
        const uint64_t begin_page = va & ~PAGE_MASK;
        const uint64_t end_page = (end + PAGE_MASK) & ~PAGE_MASK;
        for (uint32_t j = 0; j < candidate.segment_count; ++j) {
            const struct indianos_program_segment *previous = &candidate.segments[j];
            if (!disjoint(begin_page, end_page,
                          previous->page_begin, previous->page_end)) return false;
        }
        struct indianos_program_segment *seg =
            &candidate.segments[candidate.segment_count++];
        *seg = (struct indianos_program_segment) {
            .virtual_address = va, .file_offset = offset,
            .file_size = file_size, .memory_size = memory_size,
            .page_begin = begin_page, .page_end = end_page,
            .writable = (flags & ELF_PF_W) != 0,
            .executable = (flags & ELF_PF_X) != 0,
        };
        /* Entry must be backed by initialized executable file bytes. */
        if (seg->executable && entry >= va && entry - va < file_size) {
            executable_entry = true;
        }
    }
    if (candidate.segment_count == 0 || !executable_entry) return false;
    *plan = candidate; /* Commit only complete, validated plans. */
    return true;
}

bool indianos_program_user_range(const struct indianos_program_plan *plan,
                                 uint64_t address, uint64_t length, bool writable) {
    if (plan == NULL || length == 0 || address < INDIANOS_USER_MIN ||
        address >= INDIANOS_USER_TOP || length > INDIANOS_USER_TOP - address) {
        return false;
    }
    if (address >= plan->stack_begin && length <= plan->stack_end - address) return true;
    for (uint32_t i = 0; i < plan->segment_count &&
                          i < INDIANOS_MAX_PROGRAM_SEGMENTS; ++i) {
        const struct indianos_program_segment *seg = &plan->segments[i];
        if (address >= seg->page_begin &&
            address < seg->page_end && length <= seg->page_end - address &&
            (!writable || seg->writable)) return true;
    }
    return false;
}

bool indianos_program_self_test(void) {
    /* A fictional ELF fixture, never executed or loaded into ring 3. */
    static const uint8_t tiny[0x110] = {
        [0] = 0x7f, [1] = 'E', [2] = 'L', [3] = 'F',
        [4] = 2, [5] = 1, [6] = 1,
        [16] = 2, [18] = 62, [20] = 1,
        [24] = 0x00, [25] = 0x01, [26] = 0x40,
        [32] = 64, [52] = 64, [54] = 56, [56] = 1,
        [64] = 1, [68] = 5,
        [72] = 0x00, [73] = 0x01,
        [80] = 0x00, [81] = 0x01, [82] = 0x40,
        [96] = 1, [104] = 0x00, [105] = 0x10,
        [112] = 0x00, [113] = 0x10,
        [256] = 0xc3,
    };
    struct indianos_program_plan plan = {0};
    return indianos_program_plan_elf64(tiny, sizeof tiny, &plan) &&
           plan.entry == 0x400100 && plan.segment_count == 1 &&
           plan.segments[0].executable && !plan.segments[0].writable &&
           indianos_program_user_range(&plan, plan.entry, 1, false) &&
           !indianos_program_user_range(&plan, plan.entry, 1, true) &&
           !indianos_program_user_range(&plan, plan.guard_begin, 1, false);
}
