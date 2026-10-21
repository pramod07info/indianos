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
        case INDIANOS_SYS_YIELD:
            return request.arg0 == 0 && request.arg1 == 0;
        default:
            return false;
    }
}

bool indianos_syscall_self_test(void) {
    const struct indianos_program_plan plan = {
        .stack_begin = INDIANOS_USER_TOP - INDIANOS_USER_STACK_BYTES,
        .stack_end = INDIANOS_USER_TOP,
        .guard_begin = INDIANOS_USER_TOP - INDIANOS_USER_STACK_BYTES - INDIANOS_USER_PAGE_BYTES,
    };
    return indianos_syscall_validate(&plan,
               (struct indianos_syscall_request){INDIANOS_SYS_ABI_QUERY, 0, 0}) &&
           indianos_syscall_validate(&plan,
               (struct indianos_syscall_request){INDIANOS_SYS_WRITE, plan.stack_begin, 8}) &&
           !indianos_syscall_validate(&plan,
               (struct indianos_syscall_request){INDIANOS_SYS_WRITE, plan.guard_begin, 1}) &&
           indianos_syscall_validate(&plan,
               (struct indianos_syscall_request){INDIANOS_SYS_EXIT, 0, 0}) &&
           indianos_syscall_validate(&plan,
               (struct indianos_syscall_request){INDIANOS_SYS_YIELD, 0, 0}) &&
           !indianos_syscall_validate(&plan,
               (struct indianos_syscall_request){INDIANOS_SYS_YIELD, 1, 0}) &&
           !indianos_syscall_validate(&plan,
               (struct indianos_syscall_request){999, 0, 0});
}
