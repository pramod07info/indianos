/* R014.2 IRQ1 entry, included only with feature test-irq-keyboard. */
.text
/* R014.2 legacy keyboard IRQ1 at IDT 0x21. Save every GPR so the
 * interrupted CPL3 code resumes intact after the Rust IRQ handler. */
.global x86_64_keyboard_interrupt
.type x86_64_keyboard_interrupt,@function
x86_64_keyboard_interrupt:
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
    call x86_64_keyboard_irq_dispatch
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
.size x86_64_keyboard_interrupt, .-x86_64_keyboard_interrupt

.section .note.GNU-stack,"",@progbits
