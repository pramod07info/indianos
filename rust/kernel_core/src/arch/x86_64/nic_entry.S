/* IRQ11 notification only; preserve kernel registers and arbitrary RSP alignment. */
.text
.global x86_64_nic_irq11
.type x86_64_nic_irq11,@function
x86_64_nic_irq11:
    cld
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
    movq %rsp, %r12
    andq $-16, %rsp
    call indianos_fx_enter_kernel
    call x86_64_nic_irq_dispatch
    movq %r12, %rsp
    testb $3, 128(%rsp)
    jz 1f
    btrq $16, 136(%rsp)
    call indianos_fx_enter_user
1:
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
.size x86_64_nic_irq11, .-x86_64_nic_irq11
.section .note.GNU-stack,"",@progbits
