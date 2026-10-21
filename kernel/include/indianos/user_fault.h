#ifndef INDIANOS_USER_FAULT_H
#define INDIANOS_USER_FAULT_H

#include <stdbool.h>
#include <stdint.h>

/* Only ring-3 code-selector faults from an active demo process may be
 * recovered. Reserved-bit (#PF RSVD) failures remain fatal kernel faults. */
#define INDIANOS_USER_CODE_SELECTOR 0x2bull
#define INDIANOS_USER_FAULT_EXIT_BASE 0x100ull
#define INDIANOS_USER_VECTOR_UD 6ull
#define INDIANOS_USER_VECTOR_GP 13ull
#define INDIANOS_USER_VECTOR_PF 14ull

bool indianos_user_fault_eligible(uint64_t vector, uint64_t cs,
                                  uint64_t error_code, bool active);

/* Returns only for faults not owned by a running ring-3 user program.
 * Eligible faults terminate the user task and jump to the saved kernel
 * continuation, avoiding IRETQ to the invalid user instruction. */
void x86_64_user_fault_handle(uint64_t vector, uint64_t cs,
                              uint64_t error_code, uint64_t cr2);

#endif
