#include <indianos/user_image.h>
#include <indianos/vmm.h>
#include <stddef.h>
#include <stdint.h>

#define PAGE_BYTES INDIANOS_USER_PAGE_BYTES

static void zero_frame(void *ptr) {
    volatile uint8_t *buffer = ptr;
    for (size_t i = 0; i < PAGE_BYTES; ++i) buffer[i] = 0;
}
static void copy_bytes(void *destination, const uint8_t *source, size_t count) {
    volatile uint8_t *dst = destination;
    for (size_t i = 0; i < count; ++i) dst[i] = source[i];
}
static bool stage_page(struct indianos_user_image *image, uint64_t va,
                       struct vmm_permissions permissions,
                       uint64_t *physical_out) {
    if (image->page_count >= INDIANOS_USER_MAX_PAGES) return false;
    const struct vmm_backend *b = &image->tables.backend;
    uint64_t physical = 0;
    if (!b->allocate(&physical, b->context)) return false;
    void *mapped_frame = b->access(physical, b->context);
    if (mapped_frame == NULL) {
        (void)b->release(physical, b->context);
        return false;
    }
    zero_frame(mapped_frame);
    if (!vmm_map_page(&image->tables, va, physical, permissions)) {
        (void)b->release(physical, b->context);
        return false;
    }
    image->pages[image->page_count++] = (struct indianos_user_page) {
        .virtual_address = va, .physical_address = physical,
    };
    *physical_out = physical;
    return true;
}

bool indianos_user_image_destroy(struct indianos_user_image *image) {
    if (image == NULL || !image->tables.initialized) return false;
    bool ok = true;
    for (size_t i = image->page_count; i > 0; --i) {
        const struct indianos_user_page page = image->pages[i - 1];
        if (!vmm_unmap_page(&image->tables, page.virtual_address)) {
            ok = false;
            continue;
        }
        if (!image->tables.backend.release(page.physical_address,
                                           image->tables.backend.context)) {
            ok = false;
        }
    }
    if (!ok) return false; /* Keep state for diagnosis; do not free live root. */
    if (!vmm_destroy_empty(&image->tables)) return false;
    image->page_count = 0;
    image->ready = false;
    image->plan = (struct indianos_program_plan) {0};
    return true;
}

bool indianos_user_image_load(struct indianos_user_image *image,
                              const struct vmm_backend *backend,
                              const uint8_t *elf, size_t size) {
    if (image == NULL || backend == NULL || image->tables.initialized ||
        image->ready) return false;
    struct indianos_program_plan plan = {0};
    if (!indianos_program_plan_elf64(elf, size, &plan)) return false;

    uint64_t requested = INDIANOS_USER_STACK_BYTES / PAGE_BYTES;
    for (uint32_t i = 0; i < plan.segment_count; ++i) {
        const struct indianos_program_segment *s = &plan.segments[i];
        uint64_t page_count = (s->page_end - s->page_begin) / PAGE_BYTES;
        if (page_count > INDIANOS_USER_MAX_PAGES - requested) return false;
        requested += page_count;
    }
    if (!vmm_create(&image->tables, backend)) return false;
    image->plan = plan;
    image->page_count = 0;
    bool ok = true;
    for (uint32_t i = 0; i < plan.segment_count && ok; ++i) {
        const struct indianos_program_segment *segment = &plan.segments[i];
        const struct vmm_permissions perm = {
            .writable = segment->writable,
            .executable = segment->executable,
            .user = true,
        };
        for (uint64_t va = segment->page_begin; va < segment->page_end;
             va += PAGE_BYTES) {
            uint64_t physical = 0;
            if (!stage_page(image, va, perm, &physical)) {
                ok = false;
                break;
            }
            const uint64_t from = va > segment->virtual_address ? va :
                                  segment->virtual_address;
            const uint64_t end_file = segment->virtual_address + segment->file_size;
            const uint64_t page_end = va + PAGE_BYTES;
            const uint64_t to = end_file < page_end ? end_file : page_end;
            if (from < to) {
                uint8_t *frame = image->tables.backend.access(
                    physical, image->tables.backend.context);
                if (frame == NULL) {
                    ok = false;
                    break;
                }
                copy_bytes(frame + (size_t)(from - va),
                           elf + (size_t)segment->file_offset +
                           (size_t)(from - segment->virtual_address),
                           (size_t)(to - from));
            }
        }
    }
    const struct vmm_permissions stack_perm = {
        .writable = true, .executable = false, .user = true,
    };
    for (uint64_t va = plan.stack_begin; va < plan.stack_end && ok;
         va += PAGE_BYTES) {
        uint64_t physical = 0;
        ok = stage_page(image, va, stack_perm, &physical);
    }
    struct vmm_mapping query = {0};
    if (ok) {
        /* Fail closed if the guard page is already mapped. */
        ok = !vmm_query_page(&image->tables, plan.guard_begin, &query) &&
             !vmm_query_page(&image->tables, 0, &query) &&
             vmm_query_page(&image->tables, plan.entry & ~(PAGE_BYTES - 1), &query) &&
             query.permissions.user && query.permissions.executable &&
             !query.permissions.writable;
    }
    if (!ok) {
        (void)indianos_user_image_destroy(image);
        return false;
    }
    image->ready = true;
    return true;
}
