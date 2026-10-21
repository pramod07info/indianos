/* QEMU-only malicious user program: attempts supervisor memory access
 * or reads from the unmapped user stack guard page. Neither can succeed.
 * If the access unexpectedly succeeds, exit status 99 fails the test.
 */
.text
.global _start
.type _start,@function
_start:
#ifdef INDIANOS_TEST_FAULT_GUARD
    /* INDIANOS_USER_TOP - 64 KiB stack - 4 KiB guard = guard page. */
    movabsq $0x00007fffeffef000, %rax
#else
    /* Known mapped supervisor-only kernel text, never accessible in ring 3. */
    movabsq $0xffffffff80000000, %rax
#endif
    movq (%rax), %rax
    /* A working kernel privilege boundary must fault before reaching this. */
    movq $2, %rax
    movq $99, %rdi
    xorq %rsi, %rsi
    int $0x80
1:  pause
    jmp 1b
.size _start, .-_start
.section .note.GNU-stack,"",@progbits
