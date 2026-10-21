#ifndef INDIANOS_USER_MODE_H
#define INDIANOS_USER_MODE_H
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

/* QEMU-only user runner; no timer/preemption/SMP. The caller owns the PCB,
 * the runner owns transient mappings and restores the bootloader CR3 before
 * destroying or reaping the user image. */
struct indianos_process;
bool x86_64_user_process_run(struct indianos_process *process,
                             const uint8_t *elf, size_t size);
bool x86_64_user_demo_run(const uint8_t *elf, size_t size);

/* Pure framing policy: host-testable syscall request and user buffer copy.
 * This is NOT an arbitrary kernel/user memcpy function. */
struct indianos_user_image;
bool indianos_user_copy_readonly(const struct indianos_user_image *image,
                                 uint64_t address, void *destination, size_t length);

#endif
