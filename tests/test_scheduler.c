#include <indianos/scheduler.h>
#include <assert.h>
#include <stdio.h>
#include <stdint.h>

int main(void) {
    struct indianos_process_table table = {0};
    struct indianos_scheduler sched = {0};
    assert(!indianos_scheduler_init(NULL, &table));
    assert(!indianos_scheduler_init(&sched, &table));
    assert(indianos_process_table_init(&table));
    struct indianos_process *a = indianos_process_create(&table);
    struct indianos_process *b = indianos_process_create(&table);
    assert(a && b && a->pid == 1 && b->pid == 2);
    assert(indianos_process_ready(a) && indianos_process_ready(b));
    assert(indianos_scheduler_init(&sched, &table));
    assert(indianos_scheduler_next(&sched) == NULL); /* no owned roots */
    /* Preparing a separate address space owns a CR3 before first entry. */
    assert(indianos_process_bind(a, 0x1000));
    assert(indianos_process_bind(b, 0x2000));
    assert(indianos_scheduler_next(&sched) == a);
    assert(indianos_process_start(a, 0x1000));
    assert(!indianos_scheduler_yield(&sched, b)); /* wrong state */
    assert(indianos_scheduler_yield(&sched, a));
    assert(a->state == INDIANOS_PROCESS_READY && a->address_space_root == 0x1000);
    assert(indianos_scheduler_next(&sched) == b);
    assert(indianos_process_start(b, 0x2000));
    assert(indianos_scheduler_yield(&sched, b));
    assert(indianos_scheduler_next(&sched) == a);
    assert(indianos_process_resume(a));
    assert(indianos_process_exit(a, 0));
    assert(indianos_process_reap(a));
    assert(indianos_scheduler_next(&sched) == b);
    assert(indianos_process_resume(b));
    assert(indianos_process_exit(b, 0));
    assert(indianos_process_reap(b));
    assert(indianos_scheduler_next(&sched) == NULL);
    assert(indianos_scheduler_all_terminal(&sched));
    assert(sched.dispatch_count == 4 && sched.yield_count == 2);
    assert(sched.tick_count == 0);
    indianos_scheduler_account_tick(&sched);
    indianos_scheduler_account_tick(&sched);
    assert(sched.tick_count == 2);
    assert(!indianos_scheduler_yield(&sched, a));
    assert(!indianos_process_bind(a, 0x3000));
    assert(!indianos_process_resume(a));

    struct indianos_process outsider = {
        .pid = 99, .state = INDIANOS_PROCESS_RUNNING, .address_space_root = 0x4000,
    };
    assert(!indianos_scheduler_yield(&sched, &outsider));
    struct indianos_process_table limits = {0};
    assert(indianos_process_table_init(&limits));
    struct indianos_process *only = indianos_process_create(&limits);
    assert(only && indianos_process_ready(only));
    assert(indianos_process_bind(only, 0x1000));
    struct indianos_scheduler maxsched = {0};
    assert(indianos_scheduler_init(&maxsched, &limits));
    for (unsigned i = 0; i < INDIANOS_SCHEDULER_MAX_DISPATCHES; ++i) {
        assert(indianos_scheduler_next(&maxsched) == only);
    }
    assert(indianos_scheduler_next(&maxsched) == NULL);
    puts("IndianOS M009 round-robin scheduler host tests passed");
    return 0;
}
