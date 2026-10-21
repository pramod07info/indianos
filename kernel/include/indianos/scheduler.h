#ifndef INDIANOS_SCHEDULER_H
#define INDIANOS_SCHEDULER_H

#include <indianos/process.h>
#include <stdbool.h>
#include <stdint.h>

/* M009 single-CPU, bounded round-robin policy. No hardware IRQ assumption.
 * Run queue consists of READY PCB slots, not heap pointers. */
#define INDIANOS_SCHEDULER_MAX_DISPATCHES 32u
struct indianos_scheduler {
    struct indianos_process_table *table;
    uint32_t cursor;
    uint32_t dispatch_count;
    uint32_t yield_count;
    uint64_t tick_count;
};

bool indianos_scheduler_init(struct indianos_scheduler *scheduler,
                             struct indianos_process_table *table);
/* Fair scan begins at cursor; no eligible task gives NULL without mutation. */
struct indianos_process *indianos_scheduler_next(struct indianos_scheduler *scheduler);
/* Keep a paused task's owned CR3 root intact until it exits and is reaped. */
bool indianos_scheduler_yield(struct indianos_scheduler *scheduler,
                              struct indianos_process *process);
void indianos_scheduler_account_tick(struct indianos_scheduler *scheduler);
bool indianos_scheduler_all_terminal(const struct indianos_scheduler *scheduler);

#endif
