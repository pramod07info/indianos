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
