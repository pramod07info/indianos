#ifndef INDIANOS_USER_IMAGE_H
#define INDIANOS_USER_IMAGE_H

#include <indianos/program.h>
#include <indianos/vmm.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

/* Prototype loader: owns PMM-backed DATA frames and its DETACHED VMM tree;
 * no user program is executed. Source bytes must remain valid only for load.
 * A backend's access() must map an entire zeroable 4 KiB physical frame.
 * This prototype is single-threaded, does not support ASLR or shared pages.
 */
#define INDIANOS_USER_MAX_PAGES 128u
struct indianos_user_page {
    uint64_t virtual_address;
    uint64_t physical_address;
};
struct indianos_user_image {
    struct vmm_space tables;
    struct indianos_program_plan plan;
    struct indianos_user_page pages[INDIANOS_USER_MAX_PAGES];
    uint16_t page_count;
    bool ready;
};

bool indianos_user_image_load(struct indianos_user_image *image,
                              const struct vmm_backend *backend,
                              const uint8_t *elf, size_t size);
/* Caller must destroy images before switching active CR3; destroy does NOT
 * unmap pages from the current CPU's active address space or flush the TLB. */
bool indianos_user_image_destroy(struct indianos_user_image *image);

#endif
