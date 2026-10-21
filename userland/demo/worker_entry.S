/* M008 second, independent native IndianOS process. This is linked to a
 * separate ELF and copied into its own PMM-backed process address space. */
.text
.global _start
.type _start,@function
_start:
    movq $1, %rax
    leaq worker_message(%rip), %rdi
    movq $worker_message_end-worker_message, %rsi
    int $0x80
    cmpq $worker_message_end-worker_message, %rax
    jne worker_failed
    movq $2, %rax
    xorq %rdi, %rdi
    xorq %rsi, %rsi
    int $0x80
worker_failed:
    movq $2, %rax
    movq $2, %rdi
    xorq %rsi, %rsi
    int $0x80
1:  pause
    jmp 1b
.size _start, .-_start

.section .rodata,"a",@progbits
worker_message:
    .ascii "IndianOS worker process 2 completed"
worker_message_end:
.section .note.GNU-stack,"",@progbits
