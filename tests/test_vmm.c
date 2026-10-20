/* Exercise production VMM algorithms with a RAM-backed physical frame pool. */
#include <indianos/vmm.h>
#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

#define POOL_FRAMES 32u
#define PAGE 4096ull
struct simulated_memory {
    _Alignas(4096) uint8_t pages[POOL_FRAMES][4096];
    bool busy[POOL_FRAMES];
    unsigned int allocated;
    int fail_after; /* -1 means no fault injection. */
};

static bool allocate(uint64_t *physical, void *ctx) {
    struct simulated_memory *sim = ctx;
    if (sim->fail_after == 0) {
        return false;
    }
    if (sim->fail_after > 0) {
        --sim->fail_after;
    }
    for (unsigned int i = 0; i < POOL_FRAMES; ++i) {
        if (!sim->busy[i]) {
            sim->busy[i] = true;
            ++sim->allocated;
            memset(sim->pages[i], 0xa5, 4096); /* Must be cleared by VMM. */
            *physical = (uint64_t)(i + 1u) * PAGE;
            return true;
        }
    }
    return false;
}

static bool release(uint64_t physical, void *ctx) {
    struct simulated_memory *sim = ctx;
    if (physical == 0 || (physical & (PAGE - 1)) != 0 ||
        physical / PAGE > POOL_FRAMES) {
        return false;
    }
    unsigned int index = (unsigned int)(physical / PAGE - 1);
    if (!sim->busy[index]) {
        return false;
    }
    sim->busy[index] = false;
    --sim->allocated;
    return true;
}

static void *access_page(uint64_t physical, void *ctx) {
    struct simulated_memory *sim = ctx;
    if (physical == 0 || (physical & (PAGE - 1)) != 0 ||
        physical / PAGE > POOL_FRAMES) {
        return NULL;
    }
    unsigned int index = (unsigned int)(physical / PAGE - 1);
    return sim->busy[index] ? sim->pages[index] : NULL;
}

static struct vmm_backend backend_for(struct simulated_memory *sim) {
    return (struct vmm_backend) {
        .allocate = allocate, .release = release,
        .access = access_page, .context = sim,
    };
}

int main(void) {
    struct simulated_memory sim = {.fail_after = -1};
    struct vmm_backend backend = backend_for(&sim);
    struct vmm_space space = {0};
    struct vmm_mapping result = {0};
    const uint64_t kva = 0xfffffe0000000000ull;
    const uint64_t uva = 0x0000000040000000ull;
    const struct vmm_permissions data = {true, false, false};
    const struct vmm_permissions text = {false, true, false};
    const struct vmm_permissions userdata = {true, false, true};
    assert(!vmm_create(NULL, &backend));
    assert(!vmm_map_page(&space, kva, 0x1000, data));
    assert(vmm_create(&space, &backend));
    assert(!vmm_create(&space, &backend));
    assert(sim.allocated == 1);
    assert(!vmm_query_page(&space, kva, &result));
    assert(!vmm_unmap_page(&space, kva));
    assert(!vmm_map_page(&space, kva + 1, 0x1000, data));
    assert(!vmm_map_page(&space, kva, 0, data));
    assert(!vmm_map_page(&space, kva, 0x1234, data));
    assert(!vmm_map_page(&space, 0x0000800000000000ull, 0x1000, data));
    assert(!vmm_map_page(&space, kva, 0x1000, (struct vmm_permissions){true, true, false}));
    assert(!vmm_map_page(&space, uva, 0x1000, data));
    assert(!vmm_map_page(&space, kva, 0x1000, userdata));
    assert(vmm_map_page(&space, kva, 0x11000, data));
    assert(sim.allocated == 4);
    assert(vmm_query_page(&space, kva, &result));
    assert(result.physical == 0x11000 && result.permissions.writable &&
           !result.permissions.executable && !result.permissions.user);
    assert(!vmm_map_page(&space, kva, 0x12000, data));
    assert(vmm_map_page(&space, kva + PAGE, 0x12000, text));
    assert(sim.allocated == 4); /* Same intermediate tables are reused. */
    assert(vmm_query_page(&space, kva + PAGE, &result));
    assert(result.physical == 0x12000 && !result.permissions.writable &&
           result.permissions.executable);
    assert(!vmm_destroy_empty(&space));
    assert(vmm_unmap_page(&space, kva));
    assert(!vmm_query_page(&space, kva, &result));
    assert(sim.allocated == 4); /* Sibling still uses the tables. */
    assert(vmm_unmap_page(&space, kva + PAGE));
    assert(sim.allocated == 1); /* Intermediate tables reclaimed. */
    assert(!vmm_unmap_page(&space, kva + PAGE));
    assert(vmm_map_page(&space, uva, 0x13000, userdata));
    assert(vmm_query_page(&space, uva, &result) && result.permissions.user);
    assert(!vmm_map_page(&space, uva + PAGE, 0x14000, data));
    assert(vmm_unmap_page(&space, uva));
    assert(vmm_destroy_empty(&space));
    assert(sim.allocated == 0);

    /* Incomplete page-table construction must roll back all allocations. */
    for (int failure = 0; failure < 3; ++failure) {
        sim.fail_after = -1;
        struct vmm_space temp = {0};
        assert(vmm_create(&temp, &backend));
        sim.fail_after = failure;
        assert(!vmm_map_page(&temp, kva, 0x15000, data));
        assert(sim.allocated == 1);
        assert(!vmm_query_page(&temp, kva, &result));
        sim.fail_after = -1;
        assert(vmm_destroy_empty(&temp));
        assert(sim.allocated == 0);
    }
    sim.fail_after = 0;
    struct vmm_space failed = {0};
    assert(!vmm_create(&failed, &backend));
    assert(sim.allocated == 0);

    puts("IndianOS VMM host tests passed");
    return 0;
}
