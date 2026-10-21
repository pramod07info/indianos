/* IndianOS QEMU-only native user application (x86-64 ET_EXEC).
 * Three-argument syscall ABI: RAX number, RDI pointer/status, RSI length.
 * This assembly is freestanding and uses no Linux or Windows services.
 */
.text
.global _start
.type _start,@function
_start:
    movq $1, %rax
    leaq message(%rip), %rdi
    movq $message_end-message, %rsi
    int $0x80
    cmpq $message_end-message, %rax
    jne failed
    movq $2, %rax
    xorq %rdi, %rdi
    xorq %rsi, %rsi
    int $0x80
failed:
    movq $2, %rax
    movq $1, %rdi
    xorq %rsi, %rsi
    int $0x80
1:  pause
    jmp 1b
.size _start, .-_start

.section .rodata,"a",@progbits
message:
    .ascii "Namaste from IndianOS user mode!"
message_end:
.section .note.GNU-stack,"",@progbits
