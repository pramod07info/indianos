/* Production initramfs and user image algorithms, host-side mock PMM. */
#include <indianos/initramfs.h>
#include <indianos/user_image.h>
#include <indianos/user_mode.h>
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

#define CAPACITY 160u
#define PAGE 4096ull
struct ram {
    _Alignas(4096) uint8_t frames[CAPACITY][4096];
    bool used[CAPACITY];
    unsigned used_count;
    int fail_after;
};
static bool alloc(uint64_t *addr, void *ctx) {
    struct ram *r = ctx;
    if (r->fail_after == 0) return false;
    if (r->fail_after > 0) --r->fail_after;
    for (unsigned i = 0; i < CAPACITY; ++i) if (!r->used[i]) {
        r->used[i] = true;
        ++r->used_count;
        *addr = (i + 1ull) * PAGE;
        memset(r->frames[i], 0xa5, PAGE);
        return true;
    }
    return false;
}
static bool release(uint64_t addr, void *ctx) {
    struct ram *r = ctx;
    if (addr == 0 || addr % PAGE || addr / PAGE > CAPACITY) return false;
    unsigned i = (unsigned)(addr / PAGE - 1);
    if (!r->used[i]) return false;
    r->used[i] = false;
    --r->used_count;
    return true;
}
static void *access_frame(uint64_t addr, void *ctx) {
    struct ram *r = ctx;
    if (addr == 0 || addr % PAGE || addr / PAGE > CAPACITY) return NULL;
    unsigned i = (unsigned)(addr / PAGE - 1);
    return r->used[i] ? r->frames[i] : NULL;
}
static void w16(uint8_t *p, unsigned x) {
    p[0] = (uint8_t)x; p[1] = (uint8_t)(x >> 8);
}
static void w32(uint8_t *p, uint32_t x) {
    for (unsigned i = 0; i < 4; ++i) p[i] = (uint8_t)(x >> (i * 8));
}
static void w64(uint8_t *p, uint64_t x) {
    for (unsigned i = 0; i < 8; ++i) p[i] = (uint8_t)(x >> (i * 8));
}
static void build_elf(uint8_t *elf, size_t n) {
    memset(elf, 0, n);
    elf[0] = 0x7f; elf[1] = 'E'; elf[2] = 'L'; elf[3] = 'F';
    elf[4] = 2; elf[5] = 1; elf[6] = 1;
    w16(elf + 16, 2); w16(elf + 18, 62); w32(elf + 20, 1);
    w64(elf + 24, 0x400100); w64(elf + 32, 64);
    w16(elf + 52, 64); w16(elf + 54, 56); w16(elf + 56, 1);
    w32(elf + 64, 1); w32(elf + 68, 5);
    w64(elf + 72, 0x100); w64(elf + 80, 0x400100);
    w64(elf + 96, 128); w64(elf + 104, 0x3000);
    w64(elf + 112, 0x100);
    for (unsigned i = 0; i < 128; ++i) elf[0x100 + i] = (uint8_t)(i + 1);
}
static size_t archive(uint8_t *buf, size_t capacity,
                      const char *name, const uint8_t *payload, size_t n) {
    const size_t name_len = strlen(name), total = 16 + 8 + name_len + n;
    assert(total <= capacity);
    memset(buf, 0, total);
    memcpy(buf, "INDRAM01", 8);
    w32(buf + 8, 1); w32(buf + 12, (uint32_t)total);
    w16(buf + 16, (unsigned)name_len);
    w32(buf + 20, (uint32_t)n);
    memcpy(buf + 24, name, name_len);
    memcpy(buf + 24 + name_len, payload, n);
    return total;
}
int main(void) {
    uint8_t elf[0x200], pack[0x400];
    build_elf(elf, sizeof elf);
    const size_t bytes = archive(pack, sizeof pack, "/bin/init", elf, sizeof elf);
    struct indianos_ramfile file = {0};
    assert(indianos_initramfs_validate(pack, bytes));
    assert(indianos_initramfs_lookup(pack, bytes, "/bin/init", &file));
    assert(file.length == sizeof elf && file.bytes == pack + 33);
    assert(!indianos_initramfs_lookup(pack, bytes, "/bin/missing", &file));
    assert(!indianos_initramfs_lookup(pack, bytes, "/../init", &file));
    assert(!indianos_initramfs_validate(pack, bytes - 1));
    assert(!indianos_initramfs_validate(pack, bytes + 1));
    pack[0] = 'X';
    assert(!indianos_initramfs_validate(pack, bytes));
    pack[0] = 'I';
    pack[18] = 1;
    assert(!indianos_initramfs_validate(pack, bytes));
    pack[18] = 0;
    pack[25] = '.'; pack[26] = '.'; pack[27] = '/';
    assert(!indianos_initramfs_validate(pack, bytes));
    archive(pack, sizeof pack, "/bin/init", elf, sizeof elf);

    struct ram r = {.fail_after = -1};
    struct vmm_backend backend = {
        .allocate = alloc, .release = release, .access = access_frame,
        .context = &r
    };
    struct indianos_user_image image = {0};
    assert(indianos_user_image_load(&image, &backend, file.bytes, file.length));
    assert(image.ready && image.page_count == 20 && r.used_count > 20);
    struct vmm_mapping mapping = {0};
    assert(vmm_query_page(&image.tables, 0x400000, &mapping));
    assert(mapping.permissions.user && mapping.permissions.executable &&
           !mapping.permissions.writable);
    uint8_t *frame = access_frame(mapping.physical, &r);
    assert(frame && frame[0x100] == 1 && frame[0x17f] == 128 &&
           frame[0x180] == 0 && frame[0xfff] == 0);
    assert(vmm_query_page(&image.tables, 0x401000, &mapping));
    frame = access_frame(mapping.physical, &r);
    assert(frame && frame[0] == 0 && frame[4095] == 0);
    assert(vmm_query_page(&image.tables, image.plan.stack_begin, &mapping));
    assert(mapping.permissions.user && mapping.permissions.writable &&
           !mapping.permissions.executable);
    assert(!vmm_query_page(&image.tables, image.plan.guard_begin, &mapping));
    assert(!vmm_query_page(&image.tables, 0, &mapping));
    uint8_t copied[8] = {0};
    assert(indianos_user_copy_readonly(&image, 0x400100, copied, 4));
    assert(copied[0] == 1 && copied[1] == 2 &&
           copied[2] == 3 && copied[3] == 4);
    assert(indianos_user_copy_readonly(&image, 0x400ffe, copied, 8));
    for (unsigned i = 0; i < 8; ++i) assert(copied[i] == 0);
    assert(!indianos_user_copy_readonly(&image, image.plan.guard_begin, copied, 1));
    assert(!indianos_user_copy_readonly(&image, 0x1000, copied, 1));
    assert(!indianos_user_copy_readonly(&image, 0x400100, copied, 257));
    assert(!indianos_user_copy_readonly(&image, 0x400100, NULL, 1));
    assert(!indianos_user_image_load(&image, &backend, file.bytes, file.length));
    assert(indianos_user_image_destroy(&image));
    assert(r.used_count == 0);
    assert(!indianos_user_image_destroy(&image));

    for (int limit = 1; limit < 25; ++limit) {
        r.fail_after = limit;
        struct indianos_user_image maybe = {0};
        const bool loaded = indianos_user_image_load(
            &maybe, &backend, file.bytes, file.length);
        if (loaded) assert(indianos_user_image_destroy(&maybe));
        assert(r.used_count == 0);
    }
    r.fail_after = -1;
    struct indianos_user_image invalid = {0};
    elf[4] = 1;
    assert(!indianos_user_image_load(&invalid, &backend, elf, sizeof elf));
    assert(r.used_count == 0);
    puts("IndianOS initramfs and detached user-image staging host tests passed");
    return 0;
}
