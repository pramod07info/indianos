#ifndef INDIANOS_PMM_H
#define INDIANOS_PMM_H

#include <stdbool.h>
#include <stdint.h>
#include <indianos/memory.h>

#define INDIANOS_PAGE_SIZE 4096ull

typedef bool (*pmm_region_reader)(uint64_t index,
                                  struct indianos_memory_region *out,
                                  void *context);

/* Prototype: track the first 8 GiB of usable frames (not arbitrary RAM). */
bool pmm_initialize(uint64_t region_count, pmm_region_reader read_region,
                    void *context);
bool pmm_allocate_frame(uint64_t *physical_address);
bool pmm_release_frame(uint64_t physical_address);
uint64_t pmm_total_frames(void);
uint64_t pmm_free_frames(void);
bool pmm_self_test(void);

#endif
