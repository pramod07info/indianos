#ifndef INDIANOS_VMM_H
#define INDIANOS_VMM_H

#include <stdbool.h>
#include <stdint.h>

/* Milestone 003: builder for DETACHED x86-64 four-level address spaces.
 * No CR3 writes, TLB changes or ownership of mapped data pages are implied.
 * The backend hands out page-table frames and an accessible virtual pointer
 * to their 4 KiB contents (HHDM on the kernel, RAM buffers in unit tests).
 * All mapping operations are single-CPU, non-reentrant and not thread-safe.
 */
struct vmm_backend {
    bool (*allocate)(uint64_t *physical, void *context);
    bool (*release)(uint64_t physical, void *context);
    void *(*access)(uint64_t physical, void *context);
    void *context;
};

struct vmm_space {
    uint64_t root_physical;
    struct vmm_backend backend;
    bool initialized;
};

struct vmm_permissions {
    bool writable;
    bool executable;
    bool user;
};

struct vmm_mapping {
    uint64_t physical;
    struct vmm_permissions permissions;
};

bool vmm_create(struct vmm_space *space, const struct vmm_backend *backend);
bool vmm_map_page(struct vmm_space *space, uint64_t virtual_address,
                  uint64_t physical_address, struct vmm_permissions permissions);
bool vmm_query_page(const struct vmm_space *space, uint64_t virtual_address,
                    struct vmm_mapping *mapping);
bool vmm_unmap_page(struct vmm_space *space, uint64_t virtual_address);
/* Only succeeds with no remaining mappings; never frees mapped data frames. */
bool vmm_destroy_empty(struct vmm_space *space);

/* Execute one bounded, detached-page-table demonstration during kernel boot. */
bool vmm_boot_self_test(void);

#endif
