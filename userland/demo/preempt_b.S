/* M009 hardware-preemption test: NO SYS_YIELD.
 * CPU-bound loop must be interrupted by PIT IRQ0 while ring 3 has IF=1.
 * A separate CR3, kernel trap stack and saved GPR frame belong to this task. */
.text
.global _start
.type _start,@function
_start:
    movq $1, %rax
    leaq started(%rip), %rdi
    movq $started_end-started, %rsi
    int $0x80
    cmpq $started_end-started, %rax
    jne failed
    movq $400000000, %rcx
spin:
    decq %rcx
    jnz spin
    movq $1, %rax
    leaq finished(%rip), %rdi
    movq $finished_end-finished, %rsi
    int $0x80
    cmpq $finished_end-finished, %rax
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
started:
    .ascii "M009 timer task B started"
started_end:
finished:
    .ascii "M009 timer task B finished"
finished_end:
.section .note.GNU-stack,"",@progbits
