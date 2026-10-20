#ifndef INDIANOS_MEMORY_H
#define INDIANOS_MEMORY_H

#include <stdint.h>

/* Protocol-independent physical memory region. Type 0 means usable RAM. */
#define INDIANOS_MEMORY_USABLE 0u
struct indianos_memory_region {
    uint64_t base;
    uint64_t length;
    uint64_t type;
};

#endif
