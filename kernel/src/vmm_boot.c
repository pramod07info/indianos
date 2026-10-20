#include <indianos/boot.h>
#include <indianos/pmm.h>
#include <indianos/vmm.h>
#include <stdint.h>

struct hhdm_backend {
    uint64_t offset;
};

static bool allocate(uint64_t *physical, void *context) {
    (void)context;
    return pmm_allocate_frame(physical);
}

static bool release(uint64_t physical, void *context) {
    (void)context;
    return pmm_release_frame(physical);
}

static void *access_frame(uint64_t physical, void *context) {
    const struct hhdm_backend *backend = (const struct hhdm_backend *)context;
    if (backend == 0 || (physical & (INDIANOS_PAGE_SIZE - 1)) != 0 ||
        physical > UINT64_MAX - backend->offset) {
        return 0;
    }
    uint64_t address = physical + backend->offset;
    /* Only dereference higher-half canonical addresses. HHDM range and
     * physical address validity are asserted by Limine boot handoff. */
    uint64_t upper = address >> 48;
    if (((address & (1ull << 47)) != 0 && upper != 0xffffull) ||
        ((address & (1ull << 47)) == 0 && upper != 0)) {
        return 0;
    }
    return (void *)(uintptr_t)address;
}

bool vmm_boot_self_test(void) {
    uint64_t offset = 0;
    if (!boot_hhdm_offset(&offset) || offset == 0) {
        return false;
    }
    struct hhdm_backend hhdm = {.offset = offset};
    struct vmm_backend backend = {
        .allocate = allocate, .release = release,
        .access = access_frame, .context = &hhdm,
    };
    struct vmm_space space = {0};
    const uint64_t scratch_address = 0xfffffe0000000000ull;
    const struct vmm_permissions permissions = {
        .writable = true, .executable = false, .user = false,
    };
    const uint64_t before = pmm_free_frames();
    uint64_t data_page = 0;
    bool mapped = false;
    bool ok = vmm_create(&space, &backend) &&
              pmm_allocate_frame(&data_page);
    if (ok) {
        mapped = vmm_map_page(&space, scratch_address, data_page, permissions);
        struct vmm_mapping queried = {0};
        ok = mapped && vmm_query_page(&space, scratch_address, &queried) &&
             queried.physical == data_page && queried.permissions.writable &&
             !queried.permissions.executable && !queried.permissions.user;
    }
    if (mapped) {
        ok = vmm_unmap_page(&space, scratch_address) && ok;
    }
    if (space.initialized) {
        ok = vmm_destroy_empty(&space) && ok;
    }
    if (data_page != 0) {
        ok = pmm_release_frame(data_page) && ok;
    }
    return ok && pmm_free_frames() == before;
}
