#include <indianos/paging_clone.h>
#include <stddef.h>

#define PHYS_MASK 0x000ffffffffff000ull
#define PAGE_MASK 0xfffull
#define PAGE_PRESENT 0x001ull
#define PAGE_HUGE 0x080ull
#define PAGE_ACCESSED 0x020ull
/* Only the Accessed bit is hardware-updated at the PML4 nonleaf level. */
#define PAGE_HW_MUTABLE PAGE_ACCESSED
#define ENTRY_COUNT 512u

static bool valid_table_physical(uint64_t physical) {
    return physical != 0 && (physical & PAGE_MASK) == 0 &&
           (physical & ~PHYS_MASK) == 0;
}

static bool valid_backend(const struct paging_clone_backend *b) {
    return b != NULL && b->allocate != NULL &&
           b->release != NULL && b->access != NULL;
}

static bool canonical(uint64_t address) {
    const uint64_t high = address >> 48;
    return (address & (1ull << 47)) ? high == 0xffffull : high == 0;
}

static bool is_child(uint64_t entry, unsigned int level) {
    return level > 1 && (entry & PAGE_PRESENT) != 0 &&
           (entry & PAGE_HUGE) == 0;
}

static bool clear_page(uint64_t *table) {
    if (table == NULL) {
        return false;
    }
    for (unsigned int i = 0; i < ENTRY_COUNT; ++i) {
        table[i] = 0;
    }
    return true;
}

/* Intended only for frames allocated by this clone, never original tables. */
static bool discard_tree(const struct paging_clone_backend *b,
                         uint64_t frame, unsigned int level) {
    uint64_t *table = b->access(frame, b->context);
    if (table == NULL) {
        return false;
    }
    bool success = true;
    for (unsigned int i = 0; i < ENTRY_COUNT; ++i) {
        const uint64_t entry = table[i];
        if (is_child(entry, level)) {
            const uint64_t child = entry & PHYS_MASK;
            if (!valid_table_physical(child) ||
                !discard_tree(b, child, level - 1)) {
                success = false;
            }
        }
    }
    if (!b->release(frame, b->context)) {
        success = false;
    }
    return success;
}

static bool clone_level(const struct paging_clone_backend *b, uint64_t source,
                        unsigned int level, uint32_t *allocated,
                        uint64_t *destination, bool kernel_only) {
    if (!valid_table_physical(source) || level < 1 || level > 4 ||
        *allocated >= PAGING_CLONE_MAX_TABLES) {
        return false;
    }
    uint64_t *src = b->access(source, b->context);
    if (src == NULL) {
        return false;
    }
    uint64_t physical = 0;
    if (!b->allocate(&physical, b->context)) {
        return false;
    }
    if (!valid_table_physical(physical) || physical == source) {
        /* A broken allocator cannot be safely recovered in all cases. */
        if (valid_table_physical(physical) && physical != source) {
            (void)b->release(physical, b->context);
        }
        return false;
    }
    uint64_t *dst = b->access(physical, b->context);
    if (!clear_page(dst)) {
        (void)b->release(physical, b->context);
        return false;
    }
    ++(*allocated);
    for (unsigned int i = 0; i < ENTRY_COUNT; ++i) {
        if (kernel_only && level == 4 && i < 256u) continue;
        const uint64_t entry = src[i];
        if (level == 4 && (entry & PAGE_PRESENT) != 0 &&
            (entry & PAGE_HUGE) != 0) {
            (void)discard_tree(b, physical, level);
            return false; /* PML4 entries cannot be huge. */
        }
        if (is_child(entry, level)) {
            uint64_t new_child = 0;
            if (!clone_level(b, entry & PHYS_MASK, level - 1,
                             allocated, &new_child, kernel_only)) {
                (void)discard_tree(b, physical, level);
                return false;
            }
            dst[i] = (entry & ~PHYS_MASK) | new_child;
            if (kernel_only && level == 4) dst[i] &= ~0x4ull;
        } else {
            dst[i] = entry; /* Preserve original huge / leaf / nonpresent bits. */
            if (kernel_only && level == 4) dst[i] &= ~0x4ull;
        }
    }
    *destination = physical;
    return true;
}

bool paging_clone_root(const struct paging_clone_backend *backend,
                       uint64_t source_root, struct paging_clone_result *result) {
    if (!valid_backend(backend) || result == NULL ||
        !valid_table_physical(source_root) || result->root != 0) {
        return false;
    }
    uint64_t clone = 0;
    uint32_t allocated = 0;
    if (!clone_level(backend, source_root, 4, &allocated, &clone, false)) {
        return false;
    }
    result->root = clone;
    result->table_count = allocated;
    return true;
}


bool paging_clone_kernel_half(const struct paging_clone_backend *backend,
                              uint64_t source_root, struct paging_clone_result *result) {
    if (!valid_backend(backend) || result == NULL ||
        !valid_table_physical(source_root) || result->root != 0) {
        return false;
    }
    uint64_t clone = 0;
    uint32_t allocated = 0;
    if (!clone_level(backend, source_root, 4, &allocated, &clone, true)) {
        return false;
    }
    result->root = clone;
    result->table_count = allocated;
    return true;
}

static bool user_pml4_graft(const struct paging_clone_backend *backend,
                            uint64_t kernel_root, uint64_t user_root,
                            bool attach) {
    if (!valid_backend(backend) || !valid_table_physical(kernel_root) ||
        !valid_table_physical(user_root) || kernel_root == user_root) return false;
    uint64_t *kernel = backend->access(kernel_root, backend->context);
    const uint64_t *user = backend->access(user_root, backend->context);
    if (kernel == NULL || user == NULL) return false;
    for (unsigned i = 0; i < 256; ++i) {
        const uint64_t entry = user[i];
        if (attach) {
            if (kernel[i] != 0 || (entry != 0 &&
                ((entry & (PAGE_PRESENT | 0x4ull)) != (PAGE_PRESENT | 0x4ull) ||
                 (entry & PAGE_HUGE) != 0))) return false;
        } else if ((kernel[i] & ~PAGE_HW_MUTABLE) !=
                   (entry & ~PAGE_HW_MUTABLE)) {
            /* The CPU can set A in live
             * page-table entries after user code executes. Compare
             * frame ownership and protection flags, not HW status bits. */
            return false;
        }
    }
    /* Validate ALL entries before mutating, so failures are atomic. */
    for (unsigned i = 0; i < 256; ++i) {
        kernel[i] = attach ? user[i] : 0;
    }
    return true;
}

bool paging_attach_user_half(const struct paging_clone_backend *backend,
                             uint64_t kernel_root, uint64_t user_root) {
    return user_pml4_graft(backend, kernel_root, user_root, true);
}

bool paging_detach_user_half(const struct paging_clone_backend *backend,
                             uint64_t kernel_root, uint64_t user_root) {
    return user_pml4_graft(backend, kernel_root, user_root, false);
}

bool paging_clone_release(const struct paging_clone_backend *backend,
                          struct paging_clone_result *result) {
    if (!valid_backend(backend) || result == NULL ||
        !valid_table_physical(result->root)) {
        return false;
    }
    if (!discard_tree(backend, result->root, 4)) {
        return false;
    }
    result->root = 0;
    result->table_count = 0;
    return true;
}

bool paging_lookup(const struct paging_clone_backend *backend,
                   uint64_t root, uint64_t virtual_address,
                   uint64_t *physical, uint64_t *leaf_flags) {
    if (!valid_backend(backend) || !valid_table_physical(root) ||
        !canonical(virtual_address) || physical == NULL || leaf_flags == NULL) {
        return false;
    }
    uint64_t frame = root;
    for (unsigned int level = 4; level > 0; --level) {
        const uint64_t *table = backend->access(frame, backend->context);
        if (table == NULL) {
            return false;
        }
        const uint64_t shift = 12ull + (uint64_t)(level - 1) * 9ull;
        const uint64_t entry = table[(virtual_address >> shift) & 0x1ffu];
        if ((entry & PAGE_PRESENT) == 0 ||
            (level == 4 && (entry & PAGE_HUGE) != 0)) {
            return false;
        }
        if (level == 1 || ((entry & PAGE_HUGE) != 0 && level <= 3)) {
            const uint64_t size = 1ull << shift;
            const uint64_t base = (entry & PHYS_MASK) & ~(size - 1);
            *physical = base | (virtual_address & (size - 1));
            *leaf_flags = entry & ~PHYS_MASK;
            return true;
        }
        frame = entry & PHYS_MASK;
        if (!valid_table_physical(frame)) {
            return false;
        }
    }
    return false;
}
