#include <indianos/initramfs.h>

#define HEADER_BYTES 16u
#define ENTRY_HEADER_BYTES 8u

static uint16_t le16(const uint8_t *p) {
    return (uint16_t)((uint16_t)p[0] | ((uint16_t)p[1] << 8));
}
static uint32_t le32(const uint8_t *p) {
    return (uint32_t)p[0] | ((uint32_t)p[1] << 8) |
           ((uint32_t)p[2] << 16) | ((uint32_t)p[3] << 24);
}
static bool range_fits(size_t off, size_t len, size_t size) {
    return off <= size && len <= size - off;
}
static bool filename_ok(const uint8_t *name, size_t length) {
    if (length < 2 || length > INDIANOS_INITRAMFS_MAX_NAME ||
        name[0] != '/' || name[length - 1] == '/') {
        return false;
    }
    size_t part_length = 0;
    for (size_t i = 1; i <= length; ++i) {
        if (i == length || name[i] == '/') {
            if (part_length == 0 ||
                (part_length == 1 && name[i - 1] == '.') ||
                (part_length == 2 && name[i - 2] == '.' && name[i - 1] == '.')) {
                return false;
            }
            part_length = 0;
            continue;
        }
        const uint8_t c = name[i];
        if (!((c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') ||
              (c >= '0' && c <= '9') || c == '-' || c == '_' || c == '.')) {
            return false;
        }
        ++part_length;
    }
    return true;
}
static bool same_name(const uint8_t *a, size_t a_len,
                      const uint8_t *b, size_t b_len) {
    if (a_len != b_len) return false;
    for (size_t i = 0; i < a_len; ++i) if (a[i] != b[i]) return false;
    return true;
}
/* Walk and validate ALL entries before the caller may observe any file.
 * File pointers remain borrowed from the validated immutable input buffer. */
static bool walk(const uint8_t *buf, size_t size, const char *wanted,
                 struct indianos_ramfile *result) {
    static const char magic[] = "INDRAM01";
    if (buf == NULL || size < HEADER_BYTES || size > INDIANOS_INITRAMFS_MAX_BYTES) {
        return false;
    }
    for (unsigned i = 0; i < 8; ++i) if (buf[i] != (uint8_t)magic[i]) return false;
    const uint32_t count = le32(buf + 8);
    if (count == 0 || count > INDIANOS_INITRAMFS_MAX_FILES ||
        le32(buf + 12) != size) return false;
    /* A bounded stack table prevents duplicates without dynamic memory. */
    const uint8_t *names[INDIANOS_INITRAMFS_MAX_FILES] = {0};
    uint16_t lengths[INDIANOS_INITRAMFS_MAX_FILES] = {0};
    size_t wanted_length = 0;
    if (wanted != NULL) {
        while (wanted_length <= INDIANOS_INITRAMFS_MAX_NAME &&
               wanted[wanted_length] != '\0') ++wanted_length;
        if (wanted_length > INDIANOS_INITRAMFS_MAX_NAME ||
            !filename_ok((const uint8_t *)wanted, wanted_length)) return false;
    }
    bool found = false;
    struct indianos_ramfile candidate = {0};
    size_t cursor = HEADER_BYTES;
    for (uint32_t i = 0; i < count; ++i) {
        if (!range_fits(cursor, ENTRY_HEADER_BYTES, size)) return false;
        const uint16_t name_len = le16(buf + cursor);
        const uint16_t flags = le16(buf + cursor + 2);
        const uint32_t content_len = le32(buf + cursor + 4);
        cursor += ENTRY_HEADER_BYTES;
        if (flags != 0 || !range_fits(cursor, name_len, size) ||
            !filename_ok(buf + cursor, name_len)) return false;
        const uint8_t *name = buf + cursor;
        for (uint32_t j = 0; j < i; ++j) {
            if (same_name(names[j], lengths[j], name, name_len)) return false;
        }
        names[i] = name; lengths[i] = name_len;
        cursor += name_len;
        if (!range_fits(cursor, content_len, size)) return false;
        if (wanted != NULL && same_name(name, name_len,
                                        (const uint8_t *)wanted, wanted_length)) {
            found = true;
            candidate.bytes = buf + cursor;
            candidate.length = content_len;
        }
        cursor += content_len;
    }
    if (cursor != size) return false; /* no hidden trailing bytes */
    if (wanted != NULL && found && result != NULL) *result = candidate;
    return wanted == NULL || found;
}

bool indianos_initramfs_validate(const uint8_t *archive, size_t archive_size) {
    return walk(archive, archive_size, NULL, NULL);
}

bool indianos_initramfs_lookup(const uint8_t *archive, size_t archive_size,
                              const char *path, struct indianos_ramfile *file) {
    if (path == NULL || file == NULL) return false;
    /* Do not modify output when the archive/path is invalid. */
    return walk(archive, archive_size, path, file);
}
