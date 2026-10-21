#include <indianos/user_fault.h>

/* Deliberately excludes ring-0 faults, double faults and a page-table
 * reserved-bit violation. User #UD, #GP and #PF are terminal to the task
 * but recoverable by this single-process experimental kernel runner. */
bool indianos_user_fault_eligible(uint64_t vector, uint64_t cs,
                                  uint64_t error_code, bool active) {
    if (!active || cs != INDIANOS_USER_CODE_SELECTOR) return false;
    if (vector == INDIANOS_USER_VECTOR_PF) {
        return (error_code & 0x8ull) == 0; /* #PF RSVD is never isolated */
    }
    return vector == INDIANOS_USER_VECTOR_GP ||
           vector == INDIANOS_USER_VECTOR_UD;
}
