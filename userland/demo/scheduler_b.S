/* M009: independent ring-3 task B, voluntarily yielding twice.
 * Both tasks are mapped at the same virtual entry in distinct physical
 * address spaces, so the scheduler MUST reload the correct CR3 on resume. */
.text
.global _start
.type _start,@function
_start:
    movq $1, %rax
    leaq message1(%rip), %rdi
    movq $message1_end-message1, %rsi
    int $0x80
    cmpq $message1_end-message1, %rax
    jne task_failed
    movq $3, %rax              /* SYS_YIELD */
    xorq %rdi, %rdi
    xorq %rsi, %rsi
    int $0x80
    testq %rax, %rax          /* preserved syscall return must be zero */
    jne task_failed
    movq $1, %rax
    leaq message2(%rip), %rdi
    movq $message2_end-message2, %rsi
    int $0x80
    cmpq $message2_end-message2, %rax
    jne task_failed
    movq $3, %rax              /* SYS_YIELD */
    xorq %rdi, %rdi
    xorq %rsi, %rsi
    int $0x80
    testq %rax, %rax          /* preserved syscall return must be zero */
    jne task_failed
    movq $1, %rax
    leaq message3(%rip), %rdi
    movq $message3_end-message3, %rsi
    int $0x80
    cmpq $message3_end-message3, %rax
    jne task_failed
    movq $2, %rax             /* SYS_EXIT(0) */
    xorq %rdi, %rdi
    xorq %rsi, %rsi
    int $0x80
task_failed:
    movq $2, %rax
    movq $1, %rdi
    xorq %rsi, %rsi
    int $0x80
1:  pause
    jmp 1b
.size _start, .-_start
.section .rodata,"a",@progbits
message1:
    .ascii "M009 task B step 1"
message1_end:
message2:
    .ascii "M009 task B step 2"
message2_end:
message3:
    .ascii "M009 task B step 3"
message3_end:
.section .note.GNU-stack,"",@progbits
