#ifndef INDIANOS_SYSCALL_ABI_H
#define INDIANOS_SYSCALL_ABI_H

#include <indianos/program.h>
#include <stdbool.h>
#include <stdint.h>

/* Draft numeric contract only; not connected to an x86-64 SYSCALL gate. */
#define INDIANOS_SYSCALL_ABI 1ull
#define INDIANOS_SYS_ABI_QUERY 0ull
#define INDIANOS_SYS_WRITE 1ull
#define INDIANOS_SYS_EXIT 2ull
#define INDIANOS_MAX_SYSCALL_WRITE 256ull

struct indianos_syscall_request {
    uint64_t number;
    uint64_t arg0;
    uint64_t arg1;
};

/* Validate metadata only. Before dispatching, a real kernel must switch
 * privileges and use a fault-safe copy_from_user for SYS_WRITE. */
bool indianos_syscall_validate(const struct indianos_program_plan *plan,
                              struct indianos_syscall_request request);

#endif
