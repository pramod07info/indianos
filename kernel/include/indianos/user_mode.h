#ifndef INDIANOS_USER_MODE_H
#define INDIANOS_USER_MODE_H
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

/* QEMU-only gated experiment; refuses if paging preflight fails.
 * No scheduling, kill/recovery of faulted tasks, SMP or general app support. */
bool x86_64_user_demo_run(const uint8_t *elf, size_t size);

/* Pure framing policy: host-testable syscall request and user buffer copy.
 * This is NOT an arbitrary kernel/user memcpy function. */
struct indianos_user_image;
bool indianos_user_copy_readonly(const struct indianos_user_image *image,
                                 uint64_t address, void *destination, size_t length);

#endif
