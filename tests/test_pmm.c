/* Host-side tests: the actual allocator, not mocked allocator logic. */
#include <indianos/pmm.h>
#include <assert.h>
#include <stddef.h>
#include <stdio.h>

struct fake_map {
    struct indianos_memory_region *entries;
    uint64_t count;
};

static bool reader(uint64_t index, struct indianos_memory_region *out, void *ctx) {
    const struct fake_map *map = (const struct fake_map *)ctx;
    if (out == NULL || index >= map->count) {
        return false;
    }
    *out = map->entries[index];
    return true;
}

int main(void) {
    struct indianos_memory_region entries[] = {
        {0x0, 0x1000, 1},                /* Page 0 is reserved. */
        {0x1003, 0x5ffd, 0},             /* Align to 0x2000..0x5fff. */
        {0x7000, 0x2000, 1},            /* Reserved gap. */
        {0x9000, 0x3000, 0},            /* Three more frames. */
    };
    struct fake_map map = {entries, sizeof entries / sizeof entries[0]};
    assert(pmm_initialize(map.count, reader, &map));
    assert(pmm_total_frames() == 8);
    assert(pmm_free_frames() == 8);
    assert(!pmm_release_frame(0x2000)); /* No free before allocation. */
    assert(!pmm_release_frame(0x7000)); /* Reserved range. */
    assert(pmm_self_test());
    assert(pmm_free_frames() == 8);

    uint64_t addresses[8] = {0};
    for (unsigned i = 0; i < 8; ++i) {
        assert(pmm_allocate_frame(&addresses[i]));
        assert(addresses[i] != 0 && (addresses[i] % 4096) == 0);
        for (unsigned j = 0; j < i; ++j) {
            assert(addresses[i] != addresses[j]);
        }
    }
    assert(pmm_free_frames() == 0);
    uint64_t extra = 12345;
    assert(!pmm_allocate_frame(&extra));
    assert(extra == 12345);
    assert(!pmm_release_frame(addresses[0] + 1));
    assert(pmm_release_frame(addresses[3]));
    assert(!pmm_release_frame(addresses[3]));
    uint64_t recycled = 0;
    assert(pmm_allocate_frame(&recycled) && recycled == addresses[3]);
    for (unsigned i = 0; i < 8; ++i) {
        assert(pmm_release_frame(addresses[i]));
    }
    assert(pmm_free_frames() == 8);

    struct indianos_memory_region conflicting[] = {
        {0x1000, 0x5000, 0},
        {0x3000, 0x1000, 1},
    };
    struct fake_map bad = {conflicting, 2};
    assert(!pmm_initialize(bad.count, reader, &bad));
    assert(pmm_total_frames() == 0);
    assert(!pmm_allocate_frame(&extra));

    struct indianos_memory_region high[] = {
        {0x100000000ull, 0x3000, 0},
    };
    struct fake_map high_map = {high, 1};
    assert(pmm_initialize(high_map.count, reader, &high_map));
    assert(pmm_allocate_frame(&extra) && extra == 0x100000000ull);
    assert(pmm_release_frame(extra));

    struct indianos_memory_region zero[] = {
        {0, 0x1000, 0},
    };
    struct fake_map zero_map = {zero, 1};
    assert(!pmm_initialize(zero_map.count, reader, &zero_map));
    puts("IndianOS PMM host tests passed");
    return 0;
}
