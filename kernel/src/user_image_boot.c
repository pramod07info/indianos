/* Boot-only staging using Limine HHDM and PMM; no CR3 switch or user code. */
#include <indianos/boot.h>
#include <indianos/pmm.h>
#include <indianos/user_image.h>
#include <stdint.h>
#include <stddef.h>

struct direct_map { uint64_t offset; };
static bool alloc_frame(uint64_t *frame, void *ctx) {
    (void)ctx; return pmm_allocate_frame(frame);
}
static bool release_frame(uint64_t frame, void *ctx) {
    (void)ctx; return pmm_release_frame(frame);
}
static void *map_frame(uint64_t frame, void *ctx) {
    const struct direct_map *map = ctx;
    if (map == NULL || frame == 0 || (frame & (INDIANOS_PAGE_SIZE - 1)) ||
        frame > UINT64_MAX - map->offset) return NULL;
    const uint64_t pointer = frame + map->offset;
    if ((pointer >> 48) != 0xffffull || (pointer & (1ull << 47)) == 0)
        return NULL;
    return (void *)(uintptr_t)pointer;
}

bool user_image_boot_stage_test(const uint8_t *image, size_t image_bytes) {
    uint64_t offset = 0;
    if (!boot_hhdm_offset(&offset) || offset == 0) return false;
    struct direct_map map = {.offset = offset};
    struct vmm_backend backend = {
        .allocate = alloc_frame, .release = release_frame,
        .access = map_frame, .context = &map,
    };
    const uint64_t before = pmm_free_frames();
    struct indianos_user_image staged = {0};
    if (!indianos_user_image_load(&staged, &backend, image, image_bytes))
        return false;
    struct vmm_mapping mapping = {0};
    bool ok = vmm_query_page(&staged.tables, staged.plan.entry & ~4095ull, &mapping) &&
              mapping.permissions.user && mapping.permissions.executable &&
              !mapping.permissions.writable &&
              !vmm_query_page(&staged.tables, staged.plan.guard_begin, &mapping);
    if (!indianos_user_image_destroy(&staged)) return false;
    return ok && pmm_free_frames() == before;
}
