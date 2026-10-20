#ifndef INDIANOS_PAGING_CLONE_H
#define INDIANOS_PAGING_CLONE_H

#include <stdbool.h>
#include <stdint.h>

/* Clone only the paging structure, NOT mapped data pages. All callbacks are
 * single-threaded. The source tables must stay live for the whole clone. */
struct paging_clone_backend {
    bool (*allocate)(uint64_t *physical, void *context);
    bool (*release)(uint64_t physical, void *context);
    uint64_t *(*access)(uint64_t physical, void *context);
    void *context;
};

struct paging_clone_result {
    uint64_t root;
    uint32_t table_count;
};

/* 8192 table-frame cap to bound resource exhaustion (32 MiB). */
#define PAGING_CLONE_MAX_TABLES 8192u

bool paging_clone_root(const struct paging_clone_backend *backend,
                       uint64_t source_root, struct paging_clone_result *result);
bool paging_clone_release(const struct paging_clone_backend *backend,
                          struct paging_clone_result *result);
/* Resolve VA to PA and PTE flags; works with 4 KiB, 2 MiB, 1 GiB pages. */
bool paging_lookup(const struct paging_clone_backend *backend,
                   uint64_t root, uint64_t virtual_address,
                   uint64_t *physical, uint64_t *leaf_flags);

#endif
