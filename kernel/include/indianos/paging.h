#ifndef INDIANOS_PAGING_H
#define INDIANOS_PAGING_H

#include <stdbool.h>
#include <stdint.h>

/* CPU/platform capability snapshot. No privileged reads in the pure predicate. */
struct paging_cpu_state {
    bool long_mode;
    bool nx;
    bool msr;
    bool pae;
    bool paging_enabled;
    bool pae_enabled;
    bool la57_enabled;
    bool nxe_enabled;
    bool write_protect_enabled;
    bool pcid_enabled;
    uint8_t physical_bits;
    uint64_t cr3;
};

/* A conservative policy for 4-level x86-64 with NX. */
bool paging_preflight(const struct paging_cpu_state *state);
bool x86_64_paging_detect(struct paging_cpu_state *state);

/* Clone current mappings into an IndianOS-owned CR3 root, switch and restore.
 * Explicit QEMU-only self-test: never called in the ordinary kernel build. */
bool x86_64_paging_shadow_switch_test(void);

#endif
