#include <indianos/initramfs.h>
#include <indianos/program.h>
#include <stdio.h>
#include <stdlib.h>

int main(int argc, char **argv) {
    if (argc != 2) { fprintf(stderr, "Usage: inspect_archive file.img\n"); return 2; }
    FILE *f = fopen(argv[1], "rb");
    if (f == NULL || fseek(f, 0, SEEK_END) != 0) { if (f) fclose(f); return 2; }
    long size = ftell(f);
    if (size <= 0 || size > INDIANOS_INITRAMFS_MAX_BYTES ||
        fseek(f, 0, SEEK_SET) != 0) {
        fclose(f);
        return 2;
    }
    uint8_t *buffer = malloc((size_t)size);
    if (buffer == NULL) { fclose(f); return 2; }
    const bool read_ok = fread(buffer, 1, (size_t)size, f) == (size_t)size;
    fclose(f);
    struct indianos_ramfile file = {0};
    struct indianos_program_plan plan = {0};
    const bool ok = read_ok && indianos_initramfs_validate(buffer, (size_t)size) &&
                    indianos_initramfs_lookup(buffer, (size_t)size, "/bin/init", &file) &&
                    indianos_program_plan_elf64(file.bytes, file.length, &plan);
    if (ok) {
        printf("IndianOS initramfs contains valid /bin/init (entry 0x%llx)\n",
               (unsigned long long)plan.entry);
    } else {
        fprintf(stderr, "IndianOS initramfs or ELF rejected\n");
    }
    free(buffer);
    return ok ? 0 : 1;
}
