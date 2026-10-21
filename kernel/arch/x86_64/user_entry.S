/* x86-64 user-mode demonstration. Opt-in, single-core, no hardware IRQs.
 * NOTE: ABI and syscall gate are experimental; not a hardened OS boundary. */
.text
.extern x86_64_user_syscall_dispatch
.bss
.align 8
.global indianos_user_return_rsp
indianos_user_return_rsp: .quad 0
.global indianos_user_return_cr3
indianos_user_return_cr3: .quad 0

.text
.global x86_64_enter_user
.type x86_64_enter_user,@function
/* uint64_t x86_64_enter_user(uint64_t entry, uint64_t stack, uint64_t cr3);
 * Returns only after SYS_EXIT; all interrupts are deliberately disabled. */
x86_64_enter_user:
    pushq %rbx
    pushq %rbp
    pushq %r12
    pushq %r13
    pushq %r14
    pushq %r15
    movq %rsp, indianos_user_return_rsp(%rip)
    movq %cr3, %rax
    movq %rax, indianos_user_return_cr3(%rip)
    movq %rdx, %cr3
    movw $0x33, %ax
    movw %ax, %ds
    movw %ax, %es
    pushq $0x33              /* ring-3 SS */
    pushq %rsi               /* ring-3 RSP */
    pushq $0x02              /* IF=0, bit 1 fixed; IRQs remain masked */
    pushq $0x2b              /* ring-3 CS */
    pushq %rdi               /* ring-3 RIP */
    iretq
.size x86_64_enter_user, .-x86_64_enter_user

.global x86_64_user_interrupt
.type x86_64_user_interrupt,@function
x86_64_user_interrupt:
    cld
    /* CPU switched to TSS.RSP0 and pushed SS:RSP:RFLAGS:CS:RIP.
     * Preserve ALL GPRs before C code can execute.
     * 15 pushes = 120 bytes; TSS.RSP0 is 16-aligned, CPU frame=40,
     * then RSP is 16-aligned for the C dispatch call. */
    pushq %rax
    pushq %rbx
    pushq %rcx
    pushq %rdx
    pushq %rsi
    pushq %rdi
    pushq %rbp
    pushq %r8
    pushq %r9
    pushq %r10
    pushq %r11
    pushq %r12
    pushq %r13
    pushq %r14
    pushq %r15
    movq %rsp, %rdi
    call x86_64_user_syscall_dispatch
    movq %rax, 112(%rsp)
    popq %r15
    popq %r14
    popq %r13
    popq %r12
    popq %r11
    popq %r10
    popq %r9
    popq %r8
    popq %rbp
    popq %rdi
    popq %rsi
    popq %rdx
    popq %rcx
    popq %rbx
    popq %rax
    iretq
.size x86_64_user_interrupt, .-x86_64_user_interrupt

.global x86_64_user_exit_to_kernel
.type x86_64_user_exit_to_kernel,@function
/* noreturn on interrupt stack: restore original CR3 before releasing it. */
x86_64_user_exit_to_kernel:
    cli
    movq %rdi, %r11
    movq indianos_user_return_cr3(%rip), %rax
    movq %rax, %cr3
    movw $0x10, %ax
    movw %ax, %ds
    movw %ax, %es
    movq indianos_user_return_rsp(%rip), %rsp
    movq %r11, %rax
    popq %r15
    popq %r14
    popq %r13
    popq %r12
    popq %rbp
    popq %rbx
    retq
.size x86_64_user_exit_to_kernel, .-x86_64_user_exit_to_kernel
.section .note.GNU-stack,"",@progbits
