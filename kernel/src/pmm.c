#include <indianos/pmm.h>
#include <stddef.h>

/* A conservative boot-time bitmap allocator. It never writes to physical
 * frames and never manages Limine's reclaimable/kernel/reserved categories.
 * Independent of allocator, VM and heap: suitable for host-side C tests. */
#define MAX_PMM_SPANS 128u
#define MAX_PMM_FRAMES (8ull * 1024ull * 1024ull * 1024ull / INDIANOS_PAGE_SIZE)
#define MAX_PMM_MEMMAP_ENTRIES 4096ull

struct pmm_span {
    uint64_t base;
    uint64_t count;
    uint64_t bit_offset;
};

static struct pmm_span spans[MAX_PMM_SPANS];
static uint8_t allocated[MAX_PMM_FRAMES / 8ull];
static uint64_t span_count;
static uint64_t total_frames;
static uint64_t used_frames;
static uint64_t cursor;
static bool initialized;

static bool overlaps(uint64_t a_begin, uint64_t a_end,
                     uint64_t b_begin, uint64_t b_end) {
    return a_begin < b_end && b_begin < a_end;
}

static bool allocated_bit(uint64_t index) {
    return (allocated[index / 8u] & (uint8_t)(1u << (index % 8u))) != 0;
}

static void set_bit(uint64_t index, bool value) {
    uint8_t mask = (uint8_t)(1u << (index % 8u));
    if (value) {
        allocated[index / 8u] |= mask;
    } else {
        allocated[index / 8u] &= (uint8_t)~mask;
    }
}

/* Reset state on error too: failed initialisation must not leave free frames. */
static void reset(void) {
    initialized = false;
    span_count = total_frames = used_frames = cursor = 0;
}

bool pmm_initialize(uint64_t region_count, pmm_region_reader read_region,
                    void *context) {
    reset();
    if (read_region == NULL || region_count == 0 ||
        region_count > MAX_PMM_MEMMAP_ENTRIES) {
        return false;
    }

    for (uint64_t i = 0; i < region_count; ++i) {
        struct indianos_memory_region region;
        if (!read_region(i, &region, context) || region.length == 0 ||
            region.base > UINT64_MAX - region.length) {
            reset();
            return false;
        }
        if (region.type != INDIANOS_MEMORY_USABLE) {
            continue;
        }

        /* A page partially occupied by firmware/boot metadata is not free. */
        if (region.base > UINT64_MAX - (INDIANOS_PAGE_SIZE - 1)) {
            reset();
            return false;
        }
        uint64_t begin = (region.base + INDIANOS_PAGE_SIZE - 1) &
                         ~(INDIANOS_PAGE_SIZE - 1);
        if (begin < INDIANOS_PAGE_SIZE) {
            begin = INDIANOS_PAGE_SIZE; /* Do not allocate physical page 0. */
        }
        uint64_t end = (region.base + region.length) &
                       ~(INDIANOS_PAGE_SIZE - 1);
        if (end <= begin) {
            continue;
        }

        /* Reject overlapping usable ranges and any contradictory reserved
         * entry: safety matters more than squeezing a dubious memory map. */
        for (uint64_t j = 0; j < region_count; ++j) {
            if (i == j) {
                continue;
            }
            struct indianos_memory_region other;
            if (!read_region(j, &other, context) || other.length == 0 ||
                other.base > UINT64_MAX - other.length) {
                reset();
                return false;
            }
            if (overlaps(begin, end, other.base, other.base + other.length)) {
                reset();
                return false;
            }
        }
        if (span_count == MAX_PMM_SPANS) {
            reset();
            return false;
        }
        uint64_t count = (end - begin) / INDIANOS_PAGE_SIZE;
        if (total_frames == MAX_PMM_FRAMES) {
            break;
        }
        if (count > MAX_PMM_FRAMES - total_frames) {
            count = MAX_PMM_FRAMES - total_frames;
        }
        spans[span_count++] = (struct pmm_span) {
            .base = begin, .count = count, .bit_offset = total_frames,
        };
        total_frames += count;
    }
    if (total_frames == 0) {
        reset();
        return false;
    }
    uint64_t bytes = (total_frames + 7) / 8;
    for (uint64_t i = 0; i < bytes; ++i) {
        allocated[i] = 0;
    }
    initialized = true;
    return true;
}

static bool physical_of(uint64_t index, uint64_t *out) {
    for (uint64_t i = 0; i < span_count; ++i) {
        const struct pmm_span *span = &spans[i];
        if (index >= span->bit_offset &&
            index - span->bit_offset < span->count) {
            *out = span->base + (index - span->bit_offset) * INDIANOS_PAGE_SIZE;
            return true;
        }
    }
    return false;
}

bool pmm_allocate_frame(uint64_t *physical_address) {
    if (!initialized || physical_address == NULL || used_frames == total_frames) {
        return false;
    }
    for (uint64_t scanned = 0; scanned < total_frames; ++scanned) {
        uint64_t bit = cursor + scanned;
        if (bit >= total_frames) {
            bit -= total_frames;
        }
        if (!allocated_bit(bit)) {
            uint64_t address;
            if (!physical_of(bit, &address)) {
                return false;
            }
            set_bit(bit, true);
            ++used_frames;
            cursor = bit + 1 == total_frames ? 0 : bit + 1;
            *physical_address = address;
            return true;
        }
    }
    return false;
}

bool pmm_release_frame(uint64_t physical_address) {
    if (!initialized || (physical_address % INDIANOS_PAGE_SIZE) != 0) {
        return false;
    }
    for (uint64_t i = 0; i < span_count; ++i) {
        const struct pmm_span *span = &spans[i];
        if (physical_address < span->base) {
            continue;
        }
        uint64_t difference = physical_address - span->base;
        if (difference / INDIANOS_PAGE_SIZE >= span->count) {
            continue;
        }
        uint64_t bit = span->bit_offset + difference / INDIANOS_PAGE_SIZE;
        if (!allocated_bit(bit)) { /* Reject double-free or unallocated frames. */
            return false;
        }
        set_bit(bit, false);
        --used_frames;
        cursor = bit;
        return true;
    }
    return false;
}

uint64_t pmm_total_frames(void) {
    return initialized ? total_frames : 0;
}

uint64_t pmm_free_frames(void) {
    return initialized ? total_frames - used_frames : 0;
}

bool pmm_self_test(void) {
    if (!initialized || pmm_free_frames() < 2) {
        return false;
    }
    const uint64_t before = pmm_free_frames();
    uint64_t first = 0, second = 0, recycled = 0;
    if (!pmm_allocate_frame(&first) || !pmm_allocate_frame(&second)) {
        /* The early milestone treats this as a fatal consistency failure. */
        return false;
    }
    if (first == second || first == 0 || second == 0 ||
        !pmm_release_frame(second) ||
        !pmm_allocate_frame(&recycled) || recycled != second ||
        !pmm_release_frame(first) || !pmm_release_frame(recycled)) {
        return false;
    }
    return pmm_free_frames() == before;
}
