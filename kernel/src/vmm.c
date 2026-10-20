#include <indianos/vmm.h>
#include <stddef.h>

#define PAGE_SIZE 4096ull
#define PAGE_MASK (PAGE_SIZE - 1ull)
#define PTE_PRESENT 0x001ull
#define PTE_WRITABLE 0x002ull
#define PTE_USER 0x004ull
#define PTE_HUGE 0x080ull
#define PTE_ADDRESS 0x000ffffffffff000ull
#define PTE_NX (1ull << 63)
#define ENTRIES_PER_TABLE 512u

static bool canonical(uint64_t address) {
    uint64_t upper = address >> 48;
    return (address & (1ull << 47)) != 0 ? upper == 0xffffull : upper == 0;
}

static bool physical_page(uint64_t address) {
    return address != 0 && (address & PAGE_MASK) == 0 &&
           (address & ~PTE_ADDRESS) == 0;
}

static bool valid_virtual(uint64_t address) {
    return canonical(address) && (address & PAGE_MASK) == 0;
}

static uint16_t table_index(uint64_t virtual_address, unsigned int level) {
    return (uint16_t)((virtual_address >> (39u - level * 9u)) & 0x1ffu);
}

static bool ready(const struct vmm_space *space) {
    return space != NULL && space->initialized &&
           physical_page(space->root_physical) &&
           space->backend.allocate != NULL && space->backend.release != NULL &&
           space->backend.access != NULL;
}

static uint64_t *table_at(const struct vmm_space *space, uint64_t physical) {
    if (!physical_page(physical)) {
        return NULL;
    }
    return (uint64_t *)space->backend.access(physical, space->backend.context);
}

static bool clear_table(uint64_t *table) {
    if (table == NULL) {
        return false;
    }
    volatile uint64_t *destination = (volatile uint64_t *)table;
    for (unsigned int i = 0; i < ENTRIES_PER_TABLE; ++i) {
        destination[i] = 0;
    }
    return true;
}

static bool empty_table(const uint64_t *table) {
    if (table == NULL) {
        return false;
    }
    for (unsigned int i = 0; i < ENTRIES_PER_TABLE; ++i) {
        if (table[i] != 0) {
            return false;
        }
    }
    return true;
}

static bool new_table(struct vmm_space *space, uint64_t *physical,
                      uint64_t **virtual_table) {
    uint64_t address = 0;
    if (!space->backend.allocate(&address, space->backend.context)) {
        return false;
    }
    if (!physical_page(address)) {
        /* Invalid allocator output: never use an unaligned/zero address. */
        return false;
    }
    uint64_t *table = table_at(space, address);
    if (!clear_table(table)) {
        (void)space->backend.release(address, space->backend.context);
        return false;
    }
    *physical = address;
    *virtual_table = table;
    return true;
}

bool vmm_create(struct vmm_space *space, const struct vmm_backend *backend) {
    if (space == NULL || backend == NULL || backend->allocate == NULL ||
        backend->release == NULL || backend->access == NULL || space->initialized) {
        return false;
    }
    space->backend = *backend;
    space->root_physical = 0;
    uint64_t root = 0;
    uint64_t *root_table = NULL;
    if (!new_table(space, &root, &root_table)) {
        space->initialized = false;
        return false;
    }
    (void)root_table;
    space->root_physical = root;
    space->initialized = true;
    return true;
}

static void undo_created(struct vmm_space *space, uint64_t **entry_slots,
                         uint64_t *frames, unsigned int count) {
    while (count != 0) {
        --count;
        *entry_slots[count] = 0;
        (void)space->backend.release(frames[count], space->backend.context);
    }
}

bool vmm_map_page(struct vmm_space *space, uint64_t virtual_address,
                  uint64_t physical_address, struct vmm_permissions permissions) {
    if (!ready(space) || !valid_virtual(virtual_address) ||
        !physical_page(physical_address) ||
        (permissions.writable && permissions.executable) ||
        (permissions.user && (virtual_address & (1ull << 47)) != 0) ||
        (!permissions.user && (virtual_address & (1ull << 47)) == 0)) {
        return false;
    }
    uint64_t parent_physical = space->root_physical;
    uint64_t *created_slots[3] = {0};
    uint64_t created_frames[3] = {0};
    unsigned int created_count = 0;
    for (unsigned int level = 0; level < 3; ++level) {
        uint64_t *parent = table_at(space, parent_physical);
        if (parent == NULL) {
            undo_created(space, created_slots, created_frames, created_count);
            return false;
        }
        uint64_t *slot = &parent[table_index(virtual_address, level)];
        uint64_t entry = *slot;
        if ((entry & PTE_PRESENT) != 0) {
            if ((entry & PTE_HUGE) != 0 ||
                (((entry & PTE_USER) != 0) != permissions.user) ||
                (entry & PTE_WRITABLE) == 0 ||
                !physical_page(entry & PTE_ADDRESS)) {
                undo_created(space, created_slots, created_frames, created_count);
                return false;
            }
            parent_physical = entry & PTE_ADDRESS;
            continue;
        }
        /* A nonpresent entry with unexpected metadata is not ours to reuse. */
        if (entry != 0) {
            undo_created(space, created_slots, created_frames, created_count);
            return false;
        }
        uint64_t child_physical = 0;
        uint64_t *child_table = NULL;
        if (!new_table(space, &child_physical, &child_table)) {
            undo_created(space, created_slots, created_frames, created_count);
            return false;
        }
        (void)child_table;
        *slot = child_physical | PTE_PRESENT | PTE_WRITABLE |
                (permissions.user ? PTE_USER : 0ull);
        created_slots[created_count] = slot;
        created_frames[created_count] = child_physical;
        ++created_count;
        parent_physical = child_physical;
    }
    uint64_t *leaf_table = table_at(space, parent_physical);
    if (leaf_table == NULL) {
        undo_created(space, created_slots, created_frames, created_count);
        return false;
    }
    uint64_t *leaf = &leaf_table[table_index(virtual_address, 3)];
    if (*leaf != 0) {
        undo_created(space, created_slots, created_frames, created_count);
        return false;
    }
    *leaf = physical_address | PTE_PRESENT |
            (permissions.writable ? PTE_WRITABLE : 0ull) |
            (permissions.user ? PTE_USER : 0ull) |
            (permissions.executable ? 0ull : PTE_NX);
    return true;
}

bool vmm_query_page(const struct vmm_space *space, uint64_t virtual_address,
                    struct vmm_mapping *mapping) {
    if (!ready(space) || mapping == NULL || !valid_virtual(virtual_address)) {
        return false;
    }
    uint64_t next_physical = space->root_physical;
    for (unsigned int level = 0; level < 4; ++level) {
        uint64_t *table = table_at(space, next_physical);
        if (table == NULL) {
            return false;
        }
        uint64_t entry = table[table_index(virtual_address, level)];
        if ((entry & PTE_PRESENT) == 0 ||
            (level < 3 && (entry & PTE_HUGE) != 0) ||
            !physical_page(entry & PTE_ADDRESS)) {
            return false;
        }
        if (level == 3) {
            mapping->physical = entry & PTE_ADDRESS;
            mapping->permissions = (struct vmm_permissions) {
                .writable = (entry & PTE_WRITABLE) != 0,
                .executable = (entry & PTE_NX) == 0,
                .user = (entry & PTE_USER) != 0,
            };
            return true;
        }
        next_physical = entry & PTE_ADDRESS;
    }
    return false;
}

bool vmm_unmap_page(struct vmm_space *space, uint64_t virtual_address) {
    if (!ready(space) || !valid_virtual(virtual_address)) {
        return false;
    }
    uint64_t *tables[4] = {0};
    uint64_t frames[4] = {0};
    uint16_t indices[4] = {0};
    frames[0] = space->root_physical;
    for (unsigned int level = 0; level < 4; ++level) {
        tables[level] = table_at(space, frames[level]);
        if (tables[level] == NULL) {
            return false;
        }
        indices[level] = table_index(virtual_address, level);
        uint64_t entry = tables[level][indices[level]];
        if ((entry & PTE_PRESENT) == 0 ||
            (level < 3 && (entry & PTE_HUGE) != 0) ||
            !physical_page(entry & PTE_ADDRESS)) {
            return false;
        }
        if (level != 3) {
            frames[level + 1] = entry & PTE_ADDRESS;
        }
    }
    tables[3][indices[3]] = 0;
    /* Release only empty page-table frames; the mapped data page is CALLER owned. */
    for (unsigned int level = 3; level > 0; --level) {
        if (!empty_table(tables[level])) {
            break;
        }
        tables[level - 1][indices[level - 1]] = 0;
        if (!space->backend.release(frames[level], space->backend.context)) {
            return false;
        }
    }
    return true;
}

bool vmm_destroy_empty(struct vmm_space *space) {
    if (!ready(space)) {
        return false;
    }
    uint64_t *root = table_at(space, space->root_physical);
    if (!empty_table(root) ||
        !space->backend.release(space->root_physical, space->backend.context)) {
        return false;
    }
    space->root_physical = 0;
    space->initialized = false;
    return true;
}
