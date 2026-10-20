#include <indianos/paging.h>
#include <indianos/paging_clone.h>
#include <indianos/boot.h>
#include <indianos/memory.h>
#include <indianos/pmm.h>
#include <indianos/serial.h>
#include <stddef.h>
#include <stdint.h>

#define CR0_PG (1ull << 31)
#define CR0_WP (1ull << 16)
#define CR4_PAE (1ull << 5)
#define CR4_LA57 (1ull << 12)
#define CR4_PCIDE (1ull << 17)
#define EFER_NXE (1ull << 11)
#define EFER_LMA (1ull << 10)
#define PHYS_MASK 0x000ffffffffff000ull
#define PAGE_SIZE 4096ull

static void cpuid(uint32_t leaf, uint32_t subleaf,
                  uint32_t *a, uint32_t *b, uint32_t *c, uint32_t *d) {
    __asm__ volatile ("cpuid" : "=a"(*a), "=b"(*b), "=c"(*c), "=d"(*d)
                       : "a"(leaf), "c"(subleaf));
}

bool x86_64_paging_detect(struct paging_cpu_state *state) {
    if (state == NULL) {
        return false;
    }
    uint32_t a = 0, b = 0, c = 0, d = 0;
    cpuid(0, 0, &a, &b, &c, &d);
    if (a < 1) {
        return false;
    }
    cpuid(1, 0, &a, &b, &c, &d);
    const bool msr = (d & (1u << 5)) != 0;
    const bool pae = (d & (1u << 6)) != 0;
    if (!msr) {
        return false; /* Cannot safely inspect EFER without MSR support. */
    }
    cpuid(0x80000000u, 0, &a, &b, &c, &d);
    if (a < 0x80000008u) {
        return false;
    }
    cpuid(0x80000001u, 0, &a, &b, &c, &d);
    const bool long_mode = (d & (1u << 29)) != 0;
    const bool nx = (d & (1u << 20)) != 0;
    cpuid(0x80000008u, 0, &a, &b, &c, &d);
    const uint8_t physical_bits = (uint8_t)(a & 0xffu);
    uint64_t cr0 = 0, cr3 = 0, cr4 = 0;
    uint32_t msr_lo = 0, msr_hi = 0;
    __asm__ volatile ("mov %%cr0, %0" : "=r"(cr0));
    __asm__ volatile ("mov %%cr3, %0" : "=r"(cr3));
    __asm__ volatile ("mov %%cr4, %0" : "=r"(cr4));
    __asm__ volatile ("rdmsr" : "=a"(msr_lo), "=d"(msr_hi)
                       : "c"(0xc0000080u));
    const uint64_t efer = ((uint64_t)msr_hi << 32) | msr_lo;
    *state = (struct paging_cpu_state) {
        .long_mode = long_mode && (efer & EFER_LMA) != 0,
        .nx = nx, .msr = msr, .pae = pae,
        .paging_enabled = (cr0 & CR0_PG) != 0,
        .pae_enabled = (cr4 & CR4_PAE) != 0,
        .la57_enabled = (cr4 & CR4_LA57) != 0,
        .nxe_enabled = (efer & EFER_NXE) != 0,
        .write_protect_enabled = (cr0 & CR0_WP) != 0,
        .pcid_enabled = (cr4 & CR4_PCIDE) != 0,
        .physical_bits = physical_bits,
        .cr3 = cr3,
    };
    return true;
}

struct kernel_clone_context {
    uint64_t hhdm;
};

static bool physical_frame_in_map(uint64_t frame) {
    if ((frame & (PAGE_SIZE - 1)) != 0 || frame == 0 ||
        frame > UINT64_MAX - PAGE_SIZE) {
        return false;
    }
    for (uint64_t i = 0; i < boot_memory_region_count(); ++i) {
        struct indianos_memory_region region;
        if (!boot_memory_region_at(i, &region, NULL) ||
            region.base > UINT64_MAX - region.length) {
            return false;
        }
        if (frame >= region.base && frame - region.base <= region.length &&
            region.length - (frame - region.base) >= PAGE_SIZE) {
            /* Limine types 0,5,6,8 are usable, bootloader-reclaimable,
             * executable/modules and reserved-mapped respectively. */
            return region.type == 0 || region.type == 5 ||
                   region.type == 6 || region.type == 8;
        }
    }
    return false;
}

static bool allocate_table(uint64_t *physical, void *context) {
    (void)context;
    return pmm_allocate_frame(physical);
}

static bool release_table(uint64_t physical, void *context) {
    (void)context;
    return pmm_release_frame(physical);
}

static uint64_t *access_table(uint64_t physical, void *context) {
    const struct kernel_clone_context *ctx = context;
    if (ctx == NULL || !physical_frame_in_map(physical) ||
        physical > UINT64_MAX - ctx->hhdm) {
        return NULL;
    }
    const uint64_t virtual_address = physical + ctx->hhdm;
    if ((virtual_address >> 48) != 0xffffull ||
        (virtual_address & (1ull << 47)) == 0) {
        return NULL;
    }
    return (uint64_t *)(uintptr_t)virtual_address;
}

static bool translation_unchanged(const struct paging_clone_backend *b,
                                  uint64_t old_root, uint64_t new_root,
                                  uint64_t virtual_address) {
    uint64_t a = 0, z = 0, af = 0, zf = 0;
    return paging_lookup(b, old_root, virtual_address, &a, &af) &&
           paging_lookup(b, new_root, virtual_address, &z, &zf) &&
           a == z && af == zf;
}

bool x86_64_paging_shadow_switch_test(void) {
    struct paging_cpu_state state = {0};
    uint64_t hhdm_offset = 0;
    if (!x86_64_paging_detect(&state) || !paging_preflight(&state) ||
        !boot_hhdm_offset(&hhdm_offset) || hhdm_offset == 0) {
        return false;
    }
    const uint64_t old_cr3 = state.cr3;
    const uint64_t original_root = old_cr3 & PHYS_MASK;
    const uint64_t before = pmm_free_frames();
    struct kernel_clone_context ctx = {.hhdm = hhdm_offset};
    struct paging_clone_backend backend = {
        .allocate = allocate_table, .release = release_table,
        .access = access_table, .context = &ctx,
    };
    struct paging_clone_result copy = {0};
    if (!paging_clone_root(&backend, original_root, &copy)) {
        return false;
    }
    uint64_t stack = 0;
    __asm__ volatile ("mov %%rsp, %0" : "=r"(stack));
    /* Check the code, current stack, kernel data, and HHDM of the root.
     * The exact copied entries preserve all pre-existing boot mappings. */
    const bool safe = translation_unchanged(&backend, original_root, copy.root,
                                            (uint64_t)(uintptr_t)&x86_64_paging_shadow_switch_test) &&
                      translation_unchanged(&backend, original_root, copy.root, stack) &&
                      translation_unchanged(&backend, original_root, copy.root,
                                            (uint64_t)(uintptr_t)&state) &&
                      translation_unchanged(&backend, original_root, copy.root,
                                            copy.root + hhdm_offset);
    if (!safe) {
        (void)paging_clone_release(&backend, &copy);
        return false;
    }
    /* Single-core, interrupts-off QEMU smoke test only. Preserve low CR3
     * flags/PCID and flush translations through two serial CR3 writes.
     * Release clone frames only after the original root is restored. */
    const uint64_t new_cr3 = (old_cr3 & 0xfffull) | copy.root;
    __asm__ volatile ("cli; mov %0, %%cr3" : : "r"(new_cr3) : "memory");
    __asm__ volatile ("mov %0, %%cr3" : : "r"(old_cr3 & ~(1ull << 63)) : "memory");
    return paging_clone_release(&backend, &copy) &&
           pmm_free_frames() == before;
}
