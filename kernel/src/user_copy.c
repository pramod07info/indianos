#include <indianos/user_mode.h>
#include <indianos/user_image.h>
#include <indianos/syscall_abi.h>
#include <indianos/vmm.h>
#include <stddef.h>
#include <stdint.h>
#define PAGE 4096ull

/* Copy page-at-a-time through the kernel HHDM after checking the PLANNED
 * bounds AND each actual leaf. Never dereference the user VA in ring 0. */
bool indianos_user_copy_readonly(const struct indianos_user_image *image,
                                 uint64_t address, void *destination, size_t length) {
    if (image == NULL || !image->ready || destination == NULL ||
        length == 0 || length > INDIANOS_MAX_SYSCALL_WRITE ||
        !indianos_program_user_range(&image->plan, address, length, false))
        return false;
    uint8_t *out = destination;
    size_t done = 0;
    while (done < length) {
        const uint64_t at = address + done;
        const uint64_t page_va = at & ~(PAGE - 1);
        const size_t offset = (size_t)(at & (PAGE - 1));
        struct vmm_mapping mapping = {0};
        if (!vmm_query_page(&image->tables, page_va, &mapping) ||
            !mapping.permissions.user) return false;
        const uint8_t *source = image->tables.backend.access(
            mapping.physical, image->tables.backend.context);
        if (source == NULL) return false;
        size_t count = PAGE - offset;
        if (count > length - done) count = length - done;
        for (size_t i = 0; i < count; ++i) out[done + i] = source[offset + i];
        done += count;
    }
    return true;
}

