#ifndef INDIANOS_INITRAMFS_H
#define INDIANOS_INITRAMFS_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

/* IndianOS initramfs v1: read-only, uncompressed; never grants trust/execution.
 * An image must have 1..16 entries and <=32 MiB total size.
 * Header (16B): "INDRAM01", little-endian entry count u32, total bytes u32.
 * Each entry: name_len u16, flags u16=0, content_size u32, then absolute
 * ASCII pathname and content bytes. All records are contiguous (no padding).
 */
#define INDIANOS_INITRAMFS_MAX_FILES 16u
#define INDIANOS_INITRAMFS_MAX_NAME 128u
#define INDIANOS_INITRAMFS_MAX_BYTES (32u * 1024u * 1024u)

struct indianos_ramfile {
    const uint8_t *bytes;
    uint32_t length;
};

bool indianos_initramfs_validate(const uint8_t *archive, size_t archive_size);
bool indianos_initramfs_lookup(const uint8_t *archive, size_t archive_size,
                              const char *path, struct indianos_ramfile *file);

#endif
