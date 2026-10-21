/* The production page-table clone/paging preflight exercised in host RAM. */
#include <indianos/paging_clone.h>
#include <indianos/paging.h>
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

#define FRAMES 48u
#define PAGE 4096ull
#define ENTRY_PRESENT 0x1ull
#define ENTRY_WRITE 0x2ull
#define ENTRY_HUGE 0x80ull
struct mock_memory {
    _Alignas(4096) uint64_t tables[FRAMES][512];
    bool used[FRAMES];
    uint32_t outstanding;
    int fail_after;
};

static bool alloc_frame(uint64_t *out, void *context) {
    struct mock_memory *mem = context;
    if (mem->fail_after == 0) return false;
    if (mem->fail_after > 0) --mem->fail_after;
    for (uint32_t i = 0; i < FRAMES; ++i) {
        if (!mem->used[i]) {
            mem->used[i] = true;
            mem->outstanding++;
            *out = (i + 1ull) * PAGE;
            return true;
        }
    }
    return false;
}
static bool free_frame(uint64_t physical, void *context) {
    struct mock_memory *mem = context;
    if (physical == 0 || physical % PAGE != 0 || physical > FRAMES * PAGE) {
        return false;
    }
    const uint64_t idx = physical / PAGE - 1;
    if (!mem->used[idx]) return false;
    mem->used[idx] = false;
    mem->outstanding--;
    return true;
}
static uint64_t *get_frame(uint64_t physical, void *context) {
    struct mock_memory *mem = context;
    if (physical == 0 || physical % PAGE != 0 || physical > FRAMES * PAGE) {
        return NULL;
    }
    const uint64_t idx = physical / PAGE - 1;
    return mem->used[idx] ? mem->tables[idx] : NULL;
}
static void reserve_sources(struct mock_memory *mem) {
    for (uint32_t i = 0; i < 4; ++i) {
        mem->used[i] = true;
        mem->outstanding++;
    }
    mem->tables[0][256] = 0x2000 | ENTRY_PRESENT | ENTRY_WRITE;
    mem->tables[1][0] = 0x3000 | ENTRY_PRESENT | ENTRY_WRITE;
    mem->tables[1][1] = 0x40000000ull | ENTRY_PRESENT | ENTRY_WRITE | ENTRY_HUGE;
    mem->tables[2][0] = 0x4000 | ENTRY_PRESENT | ENTRY_WRITE;
    mem->tables[2][1] = 0x200000ull | ENTRY_PRESENT | ENTRY_WRITE | ENTRY_HUGE;
    mem->tables[3][1] = 0x600000ull | ENTRY_PRESENT | ENTRY_WRITE | (1ull << 63);
}

static void check_policy(void) {
    struct paging_cpu_state s = {
        .long_mode = true, .nx = true, .msr = true, .pae = true,
        .paging_enabled = true, .pae_enabled = true, .nxe_enabled = true,
        .write_protect_enabled = true, .physical_bits = 48,
        .cr3 = 0x3000,
    };
    assert(!paging_preflight(NULL));
    assert(paging_preflight(&s));
    s.nxe_enabled = false; assert(!paging_preflight(&s)); s.nxe_enabled = true;
    s.nx = false; assert(!paging_preflight(&s)); s.nx = true;
    s.la57_enabled = true; assert(!paging_preflight(&s)); s.la57_enabled = false;
    s.write_protect_enabled = false; assert(!paging_preflight(&s)); s.write_protect_enabled = true;
    s.pae_enabled = false; assert(!paging_preflight(&s)); s.pae_enabled = true;
    s.physical_bits = 35; assert(!paging_preflight(&s)); s.physical_bits = 48;
    s.cr3 = 0; assert(!paging_preflight(&s)); s.cr3 = 0x3000;
    s.cr3 = 1ull << 49; assert(!paging_preflight(&s)); s.cr3 = 0x3000;
    assert(paging_preflight(&s));
}

int main(void) {
    check_policy();
    struct mock_memory mem = {.fail_after = -1};
    struct paging_clone_backend b = {
        .allocate = alloc_frame, .release = free_frame,
        .access = get_frame, .context = &mem,
    };
    const uint64_t high = 0xffff800000000000ull;
    uint64_t original[4][512];
    reserve_sources(&mem);
    memcpy(original, mem.tables, sizeof original);
    uint64_t physical = 0, flags = 0;
    assert(paging_lookup(&b, 0x1000, high + 0x1123, &physical, &flags));
    assert(physical == 0x600123 && (flags & (1ull << 63)) != 0);
    assert(paging_lookup(&b, 0x1000, high + 0x201111, &physical, &flags));
    assert(physical == 0x201111);
    assert(paging_lookup(&b, 0x1000, high + 0x40001111, &physical, &flags));
    assert(physical == 0x40001111);
    assert(!paging_lookup(&b, 0x1000, 0x800000000000ull, &physical, &flags));
    assert(!paging_lookup(&b, 0x1000, high + 0x9100, &physical, &flags));
    struct paging_clone_result clone = {0};
    assert(paging_clone_root(&b, 0x1000, &clone));
    assert(clone.table_count == 4 && mem.outstanding == 8);
    assert(!paging_clone_root(&b, 0x1000, &clone));
    assert(clone.root != 0x1000);
    assert(paging_lookup(&b, clone.root, high + 0x1123, &physical, &flags));
    assert(physical == 0x600123 && (flags & (1ull << 63)) != 0);
    assert(paging_lookup(&b, clone.root, high + 0x201111, &physical, &flags));
    assert(physical == 0x201111);
    assert(paging_lookup(&b, clone.root, high + 0x40001111, &physical, &flags));
    assert(physical == 0x40001111);
    assert(memcmp(original, mem.tables, sizeof original) == 0);
    assert(paging_clone_release(&b, &clone));
    assert(mem.outstanding == 4 && clone.root == 0 && clone.table_count == 0);
    assert(!paging_clone_release(&b, &clone));
    assert(memcmp(original, mem.tables, sizeof original) == 0);
    for (int limit = 0; limit < 4; ++limit) {
        mem.fail_after = limit;
        assert(!paging_clone_root(&b, 0x1000, &clone));
        assert(mem.outstanding == 4 && clone.root == 0);
        assert(memcmp(original, mem.tables, sizeof original) == 0);
    }
    mem.fail_after = -1;
    mem.tables[0][256] |= ENTRY_HUGE; /* Invalid PML4 huge-page entry. */
    assert(!paging_clone_root(&b, 0x1000, &clone));
    assert(mem.outstanding == 4);
    mem.tables[0][256] &= ~ENTRY_HUGE;
    mem.tables[0][256] = 0x100000ull | ENTRY_PRESENT | ENTRY_WRITE;
    assert(!paging_clone_root(&b, 0x1000, &clone));
    assert(mem.outstanding == 4);
    /* Protect kernel mappings by cloning only the high canonical half. */
    mem.tables[0][256] = 0x2000 | ENTRY_PRESENT | ENTRY_WRITE;
    mem.tables[0][1] = 0x2000 | ENTRY_PRESENT | ENTRY_WRITE | 0x4ull;
    struct paging_clone_result supervisor = {0};
    assert(paging_clone_kernel_half(&b, 0x1000, &supervisor));
    assert(supervisor.table_count == 4);
    assert(mem.outstanding == 8);
    assert(!paging_lookup(&b, supervisor.root, 0x8000000000ull,
                          &physical, &flags));
    assert(paging_lookup(&b, supervisor.root, high + 0x1123,
                         &physical, &flags));
    uint64_t *kernel_root = get_frame(supervisor.root, &mem);
    assert(kernel_root != NULL && (kernel_root[256] & 0x4ull) == 0);
    uint64_t temporary_user_root = 0;
    assert(alloc_frame(&temporary_user_root, &mem));
    uint64_t *user_root = get_frame(temporary_user_root, &mem);
    assert(user_root != NULL);
    memset(user_root, 0, PAGE);
    user_root[1] = 0x3000 | ENTRY_PRESENT | ENTRY_WRITE | 0x4ull;
    assert(paging_attach_user_half(&b, supervisor.root, temporary_user_root));
    assert(kernel_root[1] == user_root[1]);
    assert(!paging_attach_user_half(&b, supervisor.root, temporary_user_root));
    assert(paging_detach_user_half(&b, supervisor.root, temporary_user_root));
    assert(kernel_root[1] == 0);
    assert(!paging_detach_user_half(&b, supervisor.root, temporary_user_root));
    assert(free_frame(temporary_user_root, &mem));
    assert(paging_clone_release(&b, &supervisor));
    assert(mem.outstanding == 4);
    puts("IndianOS paging clone and preflight host tests passed");
    return 0;
}
