//! R011: CPU-trap, descriptor and ring-3 transition stubs, linked from the
//! freestanding Rust kernel archive via global_asm rather than standalone
//! assembler objects. The instruction/frame ABI is unchanged from the
//! retained kernel/arch/x86_64/*.S reference files.
//!
//! The 15 saved GPRs precede [RIP, CS, RFLAGS, RSP, SS]; fault stubs place
//! [vector, error] ahead of the CPU-pushed frame. CPU state transitions
//! must remain raw assembly, not ordinary Rust functions.
use core::arch::global_asm;

// x86 assembler must use AT&T syntax: the reference files use %register,
// $immediate and RIP-relative addressing. This is intentionally assembled
// only in the no_std target, never in host ABI/test builds.
global_asm!(
    include_str!("arch/x86_64/interrupt_entry.S"),
    options(att_syntax),
);

#[cfg(any(feature = "preempt", feature = "test-irq-keyboard"))]
global_asm!(
    include_str!("arch/x86_64/user_entry.S"),
    rflags = const 0x202,
    options(att_syntax),
);

#[cfg(not(any(feature = "preempt", feature = "test-irq-keyboard")))]
global_asm!(
    include_str!("arch/x86_64/user_entry.S"),
    rflags = const 0x02,
    options(att_syntax),
);

#[cfg(feature="test-irq-keyboard")]
global_asm!(
    include_str!("arch/x86_64/keyboard_entry.S"),
    options(att_syntax),
);

// R019.9: APIC entry is emitted only when its Rust scheduler dispatcher
// exists. Keeping it out of the default archive prevents unresolved links.
#[cfg(all(feature = "preempt", feature = "user-runtime", feature = "timer"))]
global_asm!(
    r#"
.text
.global x86_64_apic_timer_interrupt
.type x86_64_apic_timer_interrupt,@function
x86_64_apic_timer_interrupt:
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
    movq %rsp, %rdi
    call x86_64_scheduler_apic_timer_dispatch
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
.size x86_64_apic_timer_interrupt, .-x86_64_apic_timer_interrupt
"#,
    options(att_syntax),
);
