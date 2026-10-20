#include <indianos/syscall_abi.h>

bool indianos_syscall_validate(const struct indianos_program_plan *plan,
                              struct indianos_syscall_request request) {
    if (plan == 0) return false;
    switch (request.number) {
        case INDIANOS_SYS_ABI_QUERY:
            return request.arg0 == 0 && request.arg1 == 0;
        case INDIANOS_SYS_WRITE:
            return request.arg1 > 0 && request.arg1 <= INDIANOS_MAX_SYSCALL_WRITE &&
                   indianos_program_user_range(plan, request.arg0, request.arg1, false);
        case INDIANOS_SYS_EXIT:
            return request.arg1 == 0;
        default:
            return false;
    }
}
