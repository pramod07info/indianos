#ifndef INDIANOS_BOOT_H
#define INDIANOS_BOOT_H

#include <stdbool.h>
#include <stdint.h>
#include <indianos/memory.h>

/* Boot protocols are private to this module, not kernel-wide interfaces. */
bool boot_validate(void);
void boot_draw_status(void);
/* Optional direct map of physical memory; check before any dereference. */
bool boot_hhdm_offset(uint64_t *offset);

/* Adapter keeps Limine structures out of the memory manager. */
uint64_t boot_memory_region_count(void);
bool boot_memory_region_at(uint64_t index,
                           struct indianos_memory_region *out, void *context);

#endif
