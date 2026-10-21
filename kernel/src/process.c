#include <indianos/process.h>
#include <stddef.h>
#include <stdint.h>

bool indianos_process_table_init(struct indianos_process_table *table) {
    if (table == NULL) return false;
    *table = (struct indianos_process_table){0};
    table->next_pid = 1;
    return true;
}

struct indianos_process *indianos_process_create(struct indianos_process_table *table) {
    if (table == NULL || table->next_pid == 0 ||
        table->count >= INDIANOS_MAX_PROCESSES) return NULL;
    struct indianos_process *p = &table->slots[table->count++];
    *p = (struct indianos_process) {
        .pid = table->next_pid++,
        .state = INDIANOS_PROCESS_NEW,
    };
    return p;
}

bool indianos_process_ready(struct indianos_process *p) {
    if (p == NULL || p->pid == 0 || p->state != INDIANOS_PROCESS_NEW)
        return false;
    p->state = INDIANOS_PROCESS_READY;
    return true;
}

bool indianos_process_bind(struct indianos_process *p, uint64_t root) {
    if (p == NULL || p->pid == 0 || p->state != INDIANOS_PROCESS_READY ||
        p->address_space_root != 0 || p->resources_reclaimed ||
        root == 0 || (root & 4095ull) != 0) return false;
    p->address_space_root = root;
    return true;
}

bool indianos_process_start(struct indianos_process *p, uint64_t root) {
    if (p == NULL || p->pid == 0 || p->state != INDIANOS_PROCESS_READY ||
        root == 0 || (root & 4095ull) != 0 || p->resources_reclaimed ||
        (p->address_space_root != 0 && p->address_space_root != root)) return false;
    p->state = INDIANOS_PROCESS_RUNNING;
    p->address_space_root = root;
    return true;
}

bool indianos_process_pause(struct indianos_process *p) {
    if (p == NULL || p->pid == 0 || p->state != INDIANOS_PROCESS_RUNNING ||
        p->address_space_root == 0 || p->resources_reclaimed) return false;
    p->state = INDIANOS_PROCESS_READY;
    return true;
}

bool indianos_process_resume(struct indianos_process *p) {
    if (p == NULL || p->pid == 0 || p->state != INDIANOS_PROCESS_READY ||
        p->address_space_root == 0 || p->resources_reclaimed) return false;
    p->state = INDIANOS_PROCESS_RUNNING;
    return true;
}

bool indianos_process_exit(struct indianos_process *p, uint64_t status) {
    if (p == NULL || p->state != INDIANOS_PROCESS_RUNNING ||
        p->address_space_root == 0) return false;
    p->state = INDIANOS_PROCESS_EXITED;
    p->exit_status = status;
    return true;
}

bool indianos_process_fault(struct indianos_process *p, uint64_t status,
                           uint64_t vector, uint64_t error, uint64_t address) {
    if (p == NULL || p->state != INDIANOS_PROCESS_RUNNING ||
        p->address_space_root == 0 || vector == 0) return false;
    p->state = INDIANOS_PROCESS_FAULTED;
    p->exit_status = status;
    p->fault_vector = vector;
    p->fault_error = error;
    p->fault_address = address;
    return true;
}

bool indianos_process_reap(struct indianos_process *p) {
    if (p == NULL ||
        (p->state != INDIANOS_PROCESS_EXITED &&
         p->state != INDIANOS_PROCESS_FAULTED) ||
        p->resources_reclaimed || p->address_space_root == 0) return false;
    p->address_space_root = 0;
    p->resources_reclaimed = true;
    return true;
}

bool indianos_process_success(const struct indianos_process *p) {
    return p != NULL && p->pid != 0 &&
           p->state == INDIANOS_PROCESS_EXITED &&
           p->exit_status == 0 && p->fault_vector == 0 &&
           p->address_space_root == 0 && p->resources_reclaimed;
}
