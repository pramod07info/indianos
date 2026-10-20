#include <indianos/boot.h>
#include <indianos/serial.h>
#include <indianos/initramfs.h>
#include <limine.h>
#include <stddef.h>
#include <stdint.h>

/* Pinned Limine protocol revision 6. The loader changes array[2] to zero
 * when it supports that revision; keep markers and requests in one TU. */
__attribute__((used, section(".limine_requests_start")))
static volatile uint64_t requests_start[] = LIMINE_REQUESTS_START_MARKER;

__attribute__((used, section(".limine_requests")))
static volatile uint64_t base_revision[] = LIMINE_BASE_REVISION(6);

__attribute__((used, section(".limine_requests")))
static volatile struct limine_memmap_request memory_map_request = {
    .id = LIMINE_MEMMAP_REQUEST_ID, .revision = 0
};

__attribute__((used, section(".limine_requests")))
static volatile struct limine_framebuffer_request framebuffer_request = {
    .id = LIMINE_FRAMEBUFFER_REQUEST_ID, .revision = 0
};

__attribute__((used, section(".limine_requests")))
static volatile struct limine_hhdm_request hhdm_request = {
    .id = LIMINE_HHDM_REQUEST_ID, .revision = 0
};

__attribute__((used, section(".limine_requests")))
static volatile struct limine_module_request module_request = {
    .id = LIMINE_MODULE_REQUEST_ID, .revision = 0
};

__attribute__((used, section(".limine_requests_end")))
static volatile uint64_t requests_end[] = LIMINE_REQUESTS_END_MARKER;

/* Milestone 001 only validates metadata; it does not allocate or reclaim RAM. */
bool boot_validate(void) {
    if (!LIMINE_BASE_REVISION_SUPPORTED(base_revision)) {
        serial_write("IndianOS: unsupported Limine base revision\n");
        return false;
    }

    const volatile struct limine_memmap_response *map = memory_map_request.response;
    if (map == NULL || map->entries == NULL || map->entry_count == 0 ||
        map->entry_count > 4096) {
        serial_write("IndianOS: missing or oversized memory map\n");
        return false;
    }
    bool usable = false;
    for (uint64_t i = 0; i < map->entry_count; ++i) {
        const struct limine_memmap_entry *entry = map->entries[i];
        if (entry == NULL || entry->length == 0 ||
            entry->base > UINT64_MAX - entry->length) {
            serial_write("IndianOS: invalid memory region\n");
            return false;
        }
        if (entry->type == LIMINE_MEMMAP_USABLE) {
            usable = true;
        }
    }
    if (!usable) {
        serial_write("IndianOS: no usable RAM in memory map\n");
        return false;
    }
    return true;
}

bool boot_initramfs_file(const char *path, const uint8_t **data, size_t *size) {
    const volatile struct limine_module_response *response = module_request.response;
    if (path == NULL || data == NULL || size == NULL || response == NULL ||
        response->module_count != 1 || response->modules == NULL ||
        response->modules[0] == NULL) {
        return false;
    }
    const struct limine_file *module = response->modules[0];
    if (module->address == NULL || module->size < 16 ||
        module->size > INDIANOS_INITRAMFS_MAX_BYTES) return false;
    struct indianos_ramfile file = {0};
    if (!indianos_initramfs_lookup((const uint8_t *)module->address,
                                  (size_t)module->size, path, &file)) return false;
    *data = file.bytes;
    *size = file.length;
    return true;
}

bool boot_hhdm_offset(uint64_t *offset) {
    const volatile struct limine_hhdm_response *response = hhdm_request.response;
    if (offset == NULL || response == NULL) {
        return false;
    }
    *offset = response->offset;
    return true;
}

uint64_t boot_memory_region_count(void) {
    const volatile struct limine_memmap_response *map = memory_map_request.response;
    return map == NULL ? 0 : map->entry_count;
}

bool boot_memory_region_at(uint64_t index,
                           struct indianos_memory_region *out, void *context) {
    (void)context;
    const volatile struct limine_memmap_response *map = memory_map_request.response;
    if (out == NULL || map == NULL || map->entries == NULL ||
        index >= map->entry_count || map->entries[index] == NULL) {
        return false;
    }
    const struct limine_memmap_entry *region = map->entries[index];
    *out = (struct indianos_memory_region) {
        .base = region->base, .length = region->length, .type = region->type,
    };
    return true;
}

static bool framebuffer_valid(const struct limine_framebuffer *fb) {
    if (fb == NULL || fb->address == NULL || fb->width == 0 || fb->height == 0 ||
        fb->memory_model != LIMINE_FRAMEBUFFER_RGB || fb->bpp != 32 ||
        fb->pitch % 4 != 0 || fb->width > UINT64_MAX / 4 ||
        fb->pitch < fb->width * 4 || fb->height > UINT64_MAX / fb->pitch ||
        fb->height > SIZE_MAX / fb->pitch ||
        ((uintptr_t)fb->address & 3u) != 0) {
        return false;
    }
    const uint8_t sizes[3] = {fb->red_mask_size, fb->green_mask_size, fb->blue_mask_size};
    const uint8_t shifts[3] = {fb->red_mask_shift, fb->green_mask_shift, fb->blue_mask_shift};
    uint32_t occupied = 0;
    for (unsigned int i = 0; i < 3; ++i) {
        if (sizes[i] == 0 || sizes[i] > 8 || shifts[i] >= 32 ||
            (unsigned int)sizes[i] + (unsigned int)shifts[i] > 32) {
            return false;
        }
        uint32_t mask = ((1u << sizes[i]) - 1u) << shifts[i];
        if ((occupied & mask) != 0) {
            return false;
        }
        occupied |= mask;
    }
    return true;
}

static uint32_t channel(uint8_t value, uint8_t bits, uint8_t shift) {
    uint32_t maximum = (1u << bits) - 1u;
    return ((uint32_t)value * maximum / 255u) << shift;
}

static uint32_t pixel(const struct limine_framebuffer *fb,
                      uint8_t r, uint8_t g, uint8_t b) {
    return channel(r, fb->red_mask_size, fb->red_mask_shift) |
           channel(g, fb->green_mask_size, fb->green_mask_shift) |
           channel(b, fb->blue_mask_size, fb->blue_mask_shift);
}

/* A bounded tricolor graphic, not a desktop or accelerated display driver. */
void boot_draw_status(void) {
    const volatile struct limine_framebuffer_response *resp = framebuffer_request.response;
    if (resp == NULL || resp->framebuffer_count == 0 || resp->framebuffers == NULL) {
        serial_write("IndianOS: framebuffer unavailable (serial mode)\n");
        return;
    }
    struct limine_framebuffer *fb = resp->framebuffers[0];
    if (!framebuffer_valid(fb)) {
        serial_write("IndianOS: unsupported framebuffer (serial mode)\n");
        return;
    }

    uint64_t width = fb->width < 480 ? fb->width : 480;
    uint64_t height = fb->height < 120 ? fb->height : 120;
    const uint64_t start_x = (fb->width - width) / 2;
    const uint64_t start_y = (fb->height - height) / 2;
    const uint32_t saffron = pixel(fb, 255, 153, 51);
    const uint32_t white = pixel(fb, 255, 255, 255);
    const uint32_t green = pixel(fb, 19, 136, 8);
    const uint32_t navy = pixel(fb, 0, 0, 128);

    volatile uint8_t *base = (volatile uint8_t *)fb->address;
    for (uint64_t y = 0; y < height; ++y) {
        uint32_t stripe = y < height / 3 ? saffron :
                          y < (height * 2) / 3 ? white : green;
        for (uint64_t x = 0; x < width; ++x) {
            bool center_square = x >= (width / 2 > 9 ? width / 2 - 9 : 0) &&
                                 x < width / 2 + 9 &&
                                 y >= (height / 2 > 9 ? height / 2 - 9 : 0) &&
                                 y < height / 2 + 9;
            const uint64_t offset = (start_y + y) * fb->pitch + (start_x + x) * 4;
            *(volatile uint32_t *)(base + offset) = center_square ? navy : stripe;
        }
    }
    serial_write("IndianOS: framebuffer status rendered\n");
}
