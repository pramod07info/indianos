#include <indianos/scheduler.h>
#include <stddef.h>

bool indianos_scheduler_init(struct indianos_scheduler *s,
                             struct indianos_process_table *table) {
    if (s == NULL || table == NULL || table->count == 0 ||
        table->count > INDIANOS_MAX_PROCESSES) return false;
    *s = (struct indianos_scheduler){.table = table};
    return true;
}

struct indianos_process *indianos_scheduler_next(struct indianos_scheduler *s) {
    if (s == NULL || s->table == NULL ||
        s->table->count == 0 || s->table->count > INDIANOS_MAX_PROCESSES ||
        s->dispatch_count >= INDIANOS_SCHEDULER_MAX_DISPATCHES) return NULL;
    for (uint32_t offset = 0; offset < s->table->count; ++offset) {
        uint32_t index = (s->cursor + offset) % s->table->count;
        struct indianos_process *p = &s->table->slots[index];
        if (p->pid != 0 && p->state == INDIANOS_PROCESS_READY &&
            p->address_space_root != 0 && !p->resources_reclaimed) {
            s->cursor = (index + 1u) % s->table->count;
            ++s->dispatch_count;
            return p;
        }
    }
    return NULL;
}

bool indianos_scheduler_yield(struct indianos_scheduler *s,
                              struct indianos_process *p) {
    if (s == NULL || s->table == NULL || p == NULL ||
        p->state != INDIANOS_PROCESS_RUNNING ||
        p->address_space_root == 0 || p->resources_reclaimed) return false;
    bool found = false;
    for (uint32_t i = 0; i < s->table->count; ++i) {
        if (p == &s->table->slots[i]) {
            found = true;
            break;
        }
    }
    if (!found) return false;
    if (!indianos_process_pause(p)) return false;
    ++s->yield_count;
    return true;
}

void indianos_scheduler_account_tick(struct indianos_scheduler *s) {
    if (s != NULL && s->tick_count != UINT64_MAX) ++s->tick_count;
}

bool indianos_scheduler_all_terminal(const struct indianos_scheduler *s) {
    if (s == NULL || s->table == NULL ||
        s->table->count == 0 || s->table->count > INDIANOS_MAX_PROCESSES)
        return false;
    for (uint32_t i = 0; i < s->table->count; ++i) {
        const struct indianos_process *p = &s->table->slots[i];
        if (p->pid == 0 ||
            (p->state != INDIANOS_PROCESS_EXITED &&
             p->state != INDIANOS_PROCESS_FAULTED) ||
            !p->resources_reclaimed) return false;
    }
    return true;
}
