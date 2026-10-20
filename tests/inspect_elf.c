/* Host-side CLI to inspect an actual IndianOS-style static ELF fixture. */
#include <indianos/program.h>
#include <stdio.h>
#include <stdlib.h>

int main(int argc, char **argv) {
    if (argc != 2) {
        fprintf(stderr, "Usage: inspect_elf path/to/user.elf\n");
        return 2;
    }
    FILE *file = fopen(argv[1], "rb");
    if (file == NULL || fseek(file, 0, SEEK_END) != 0) {
        fprintf(stderr, "Cannot read ELF fixture\n");
        if (file != NULL) fclose(file);
        return 2;
    }
    long size = ftell(file);
    if (size <= 0 || (unsigned long)size > INDIANOS_MAX_ELF_BYTES ||
        fseek(file, 0, SEEK_SET) != 0) {
        fclose(file);
        return 2;
    }
    unsigned char *bytes = malloc((size_t)size);
    if (bytes == NULL) { fclose(file); return 2; }
    const int read_ok = fread(bytes, 1, (size_t)size, file) == (size_t)size;
    fclose(file);
    struct indianos_program_plan plan = {0};
    const int parsed = read_ok && indianos_program_plan_elf64(bytes, (size_t)size, &plan);
    free(bytes);
    if (!parsed) {
        fprintf(stderr, "Rejected invalid or unsupported static ELF64 image\n");
        return 1;
    }
    printf("IndianOS user ELF fixture accepted: entry=0x%llx, segments=%u\n",
           (unsigned long long)plan.entry, (unsigned)plan.segment_count);
    return 0;
}
