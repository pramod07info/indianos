#include <indianos/paging.h>
#include <stddef.h>

#define PHYS_MASK 0x000ffffffffff000ull

bool paging_preflight(const struct paging_cpu_state *state) {
    if (state == NULL || !state->long_mode || !state->nx || !state->msr ||
        !state->pae || !state->paging_enabled || !state->pae_enabled ||
        state->la57_enabled || !state->nxe_enabled ||
        !state->write_protect_enabled || state->physical_bits < 36 ||
        state->physical_bits > 52 || (state->cr3 & PHYS_MASK) == 0) {
        return false;
    }
    const uint64_t source = state->cr3 & PHYS_MASK;
    return (source >> state->physical_bits) == 0;
}
