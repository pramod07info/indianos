#include <indianos/process.h>
#include <assert.h>
#include <stdint.h>
#include <stdio.h>

int main(void) {
    struct indianos_process_table table;
    assert(!indianos_process_table_init(NULL));
    assert(indianos_process_table_init(&table));
    assert(table.count == 0 && table.next_pid == 1);
    assert(indianos_process_create(NULL) == NULL);
    struct indianos_process *first = indianos_process_create(&table);
    struct indianos_process *second = indianos_process_create(&table);
    assert(first != NULL && second != NULL && first != second);
    assert(first->pid == 1 && second->pid == 2);
    assert(first->state == INDIANOS_PROCESS_NEW);
    assert(!indianos_process_exit(first, 0));
    assert(!indianos_process_start(first, 0x1000));
    assert(!indianos_process_reap(first));
    assert(indianos_process_ready(first));
    assert(!indianos_process_ready(first));
    assert(!indianos_process_start(first, 0));
    assert(!indianos_process_start(first, 0x1234));
    assert(indianos_process_start(first, 0x1000));
    assert(first->state == INDIANOS_PROCESS_RUNNING);
    assert(!indianos_process_start(first, 0x2000));
    assert(indianos_process_exit(first, 0));
    assert(!indianos_process_fault(first, 0x10e, 14, 4, 0x5000));
    assert(!indianos_process_success(first)); /* CR3/table ownership still live. */
    assert(indianos_process_reap(first));
    assert(indianos_process_success(first));
    assert(!indianos_process_reap(first));
    assert(!indianos_process_exit(first, 0));

    assert(indianos_process_ready(second));
    assert(indianos_process_start(second, 0x2000));
    assert(!indianos_process_fault(second, 0x10e, 0, 4, 0xdead));
    assert(indianos_process_fault(second, 0x10e, 14, 4, 0xdead));
    assert(second->state == INDIANOS_PROCESS_FAULTED);
    assert(second->fault_address == 0xdead);
    assert(second->fault_vector == 14);
    assert(!indianos_process_success(second));
    assert(indianos_process_reap(second));
    assert(!indianos_process_success(second));
    assert(!indianos_process_ready(second));

    struct indianos_process *third = indianos_process_create(&table);
    struct indianos_process *fourth = indianos_process_create(&table);
    assert(third != NULL && fourth != NULL);
    assert(third->pid == 3 && fourth->pid == 4);
    assert(indianos_process_create(&table) == NULL);
    assert(table.count == INDIANOS_MAX_PROCESSES);
    table.next_pid = UINT32_MAX;
    assert(indianos_process_table_init(&table));
    assert(indianos_process_create(&table)->pid == 1);
    puts("IndianOS M008 process lifecycle host tests passed");
    return 0;
}
