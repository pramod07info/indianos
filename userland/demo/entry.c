/* Freestanding ELF64 fixture, intentionally NOT executed by IndianOS.
 * User-mode page tables and syscall entry/return are not implemented yet. */
__attribute__((noreturn)) void _start(void) {
    for (;;) {
        __asm__ volatile ("pause" : : : "memory");
    }
}
