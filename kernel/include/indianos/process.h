#ifndef INDIANOS_PROCESS_H
#define INDIANOS_PROCESS_H

#include <stdbool.h>
#include <stdint.h>

/* M008: bounded, single-CPU lifecycle table. Not a scheduler or job queue. */
#define INDIANOS_MAX_PROCESSES 4u

enum indianos_process_state {
    INDIANOS_PROCESS_EMPTY = 0,
    INDIANOS_PROCESS_NEW,
    INDIANOS_PROCESS_READY,
    INDIANOS_PROCESS_RUNNING,
    INDIANOS_PROCESS_EXITED,
    INDIANOS_PROCESS_FAULTED,
};

struct indianos_process {
    uint32_t pid;
    enum indianos_process_state state;
    uint64_t address_space_root; /* page-table root only while RUNNING */
    uint64_t exit_status;
    uint64_t fault_vector;
    uint64_t fault_error;
    uint64_t fault_address;
    bool resources_reclaimed;
};

struct indianos_process_table {
    struct indianos_process slots[INDIANOS_MAX_PROCESSES];
    uint32_t count;
    uint32_t next_pid;
};

/* Success never silently resets a completed/failed process or reuses a PID.
 * Transition functions reject wrong states and null objects. */
bool indianos_process_table_init(struct indianos_process_table *table);
struct indianos_process *indianos_process_create(struct indianos_process_table *table);
bool indianos_process_ready(struct indianos_process *process);
bool indianos_process_start(struct indianos_process *process, uint64_t root);
bool indianos_process_exit(struct indianos_process *process, uint64_t status);
bool indianos_process_fault(struct indianos_process *process,
                           uint64_t status, uint64_t vector,
                           uint64_t error, uint64_t address);
/* Caller must first restore original CR3 and successfully release ALL
 * process frames, page tables and borrowed PML4 entries. */
bool indianos_process_reap(struct indianos_process *process);
bool indianos_process_success(const struct indianos_process *process);

#endif
