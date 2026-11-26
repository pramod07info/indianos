/* Single-BSP input entry. Hardware primitives only; no scheduler dispatch. */
.text
/* R014.2 legacy keyboard IRQ1 at IDT 0x21. Save every GPR so the
 * interrupted CPL3 code resumes intact after the Rust IRQ handler. */
.macro INPUT_IRQ_ENTRY entry, dispatcher
.global \entry
.type \entry,@function
\entry:
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
    call indianos_fx_enter_kernel
    call \dispatcher
    testb $3, 128(%rsp)
    jz 2f
    call indianos_fx_enter_user
2:
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
    /* A real hardware interrupt may carry CPU-generated RF. The native
     * ring-3 ABI disallows debug-resume state; normalize only that trusted
     * bit, as the timer capture path does. Keep IF, DF and every GPR intact.
     * At this point RIP/CS/RFLAGS are at offsets 0/8/16. Never alter a
     * supervisor return frame or relax the INT 0x80 validation policy. */
    testb $3, 8(%rsp)
    jz 1f
    btrq $16, 16(%rsp)
1:
    iretq
.size \entry, .-\entry
.endm
INPUT_IRQ_ENTRY x86_64_keyboard_interrupt, x86_64_keyboard_irq_dispatch
.if {mouse_irq}
INPUT_IRQ_ENTRY x86_64_mouse_interrupt, x86_64_mouse_irq_dispatch
/* Architectural local-APIC spurious vector: no EOI is required. */
.global x86_64_input_spurious_interrupt
.type x86_64_input_spurious_interrupt,@function
x86_64_input_spurious_interrupt:
    iretq
.size x86_64_input_spurious_interrupt, .-x86_64_input_spurious_interrupt
.endif

.section .note.GNU-stack,"",@progbits
