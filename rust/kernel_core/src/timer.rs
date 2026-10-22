//! Research-only legacy PIT/PIC 100 Hz driver in no_std Rust.
//!
//! IRQ0 delivery under OVMF was previously not observed, including when
//! IF=1 and PIC IRR indicated a pending timer. This is a direct ABI port,
//! NOT evidence that hardware preemption works. Disabled in normal boots.
use core::{arch::asm, sync::atomic::{AtomicBool,AtomicU64,AtomicU8,Ordering}};
const PIC1_CMD:u16=0x20;
const PIC1_DATA:u16=0x21;
const PIC2_CMD:u16=0xa0;
const PIC2_DATA:u16=0xa1;
const PIT_CMD:u16=0x43;
const PIT_DATA:u16=0x40;
static TICKS:AtomicU64=AtomicU64::new(0);
static CONFIGURED:AtomicBool=AtomicBool::new(false);
// Capture firmware APIC mode before the experimental PIC routing change.
static INITIAL_APIC_MODE:AtomicU8=AtomicU8::new(0xff);
#[inline(always)]
fn outb(port:u16,byte:u8){unsafe{
    asm!("out dx, al",in("dx")port,in("al")byte,options(nostack,nomem,preserves_flags));
}}
#[inline(always)]
fn inb(port:u16)->u8 {
    let value:u8;
    unsafe{asm!("in al, dx",in("dx")port,out("al")value,
                options(nostack,nomem,preserves_flags));}
    value
}
fn io_wait(){outb(0x80,0)}
fn select_legacy_pic()->bool{
    let mut lo:u32;
    let hi:u32;
    unsafe{asm!("rdmsr",
        in("ecx")0x1bu32,out("eax")lo,out("edx")hi,
        options(nostack,preserves_flags));}
    if lo&(1<<10)!=0{return false} // don't alter x2APIC
    if lo&(1<<11)!=0{
        lo &= !(1<<11);
        unsafe{asm!("wrmsr",
            in("ecx")0x1bu32,in("eax")lo,in("edx")hi,
            options(nostack,preserves_flags));}
    }
    true
}
#[no_mangle]
pub extern "C" fn x86_64_timer_start_100hz()->bool{
    if CONFIGURED.load(Ordering::Relaxed){return false}
    INITIAL_APIC_MODE.store(x86_64_timer_apic_mode_flags(),Ordering::Relaxed);
    if !select_legacy_pic(){return false}
    outb(PIC1_CMD,0x11);io_wait();
    outb(PIC2_CMD,0x11);io_wait();
    outb(PIC1_DATA,0x20);io_wait();
    outb(PIC2_DATA,0x28);io_wait();
    outb(PIC1_DATA,0x04);io_wait();
    outb(PIC2_DATA,0x02);io_wait();
    outb(PIC1_DATA,0x01);io_wait();
    outb(PIC2_DATA,0x01);io_wait();
    outb(PIC1_DATA,0xfe); // only IRQ0
    outb(PIC2_DATA,0xff);
    outb(0x22,0x70);io_wait();
    outb(0x23,0);io_wait();
    outb(PIT_CMD,0x36);
    const DIVISOR:u16=11932;
    outb(PIT_DATA,(DIVISOR&0xff) as u8);
    outb(PIT_DATA,(DIVISOR>>8) as u8);
    TICKS.store(0,Ordering::Relaxed);
    CONFIGURED.store(true,Ordering::Relaxed);
    true
}
#[no_mangle]
pub extern "C" fn x86_64_timer_ack_irq0(){
    if !CONFIGURED.load(Ordering::Relaxed){return}
    let _=TICKS.fetch_update(Ordering::Relaxed,Ordering::Relaxed,|v|{
        (v!=u64::MAX).then_some(v+1)
    });
    outb(PIC1_CMD,0x20);
}
#[no_mangle]
pub extern "C" fn x86_64_timer_irq_count()->u64 {
    TICKS.load(Ordering::Relaxed)
}
#[no_mangle]
pub extern "C" fn x86_64_timer_pic_pending()->u8{
    if !CONFIGURED.load(Ordering::Relaxed){return 0}
    outb(PIC1_CMD,0x0a);inb(PIC1_CMD)
}
/// Read the PIC in-service register (ISR), not the interrupt request register
/// (IRR). A pending IRQ0 with ISR bit 0 clear indicates that the PIC has
/// not acknowledged delivery to the CPU. This is diagnostic only: reading
/// the command port does not acknowledge, unmask, or enable interrupts.
#[no_mangle]
pub extern "C" fn x86_64_timer_pic_in_service()->u8{
    if !CONFIGURED.load(Ordering::Relaxed){return 0}
    outb(PIC1_CMD,0x0b);
    let isr=inb(PIC1_CMD);
    // Restore IRR selection so existing pending-IRQ probes remain stable.
    outb(PIC1_CMD,0x0a);
    isr
}
/// Decode the master PIC's IRQ0 state without conflating pending and
/// in-service interrupts. This does not establish CPU IRQ delivery.
pub const fn irq0_state(irr:u8,isr:u8,mask:u8)->u8{
    ((irr&1)!=0) as u8 | ((((isr&1)!=0) as u8)<<1) |
    ((((mask&1)!=0) as u8)<<2)
}
#[no_mangle]
pub extern "C" fn x86_64_timer_irq0_state()->u8{
    irq0_state(x86_64_timer_pic_pending(),
               x86_64_timer_pic_in_service(),
               x86_64_timer_pic_mask())
}
/// IA32_APIC_BASE state used to diagnose legacy PIC routing under QEMU.
/// Bit 0: xAPIC enabled; bit 1: x2APIC enabled; bit 2: BSP.
/// This read-only probe does not modify interrupt-controller configuration.
pub const fn apic_mode_flags(msr_low:u32)->u8{
    (((msr_low>>11)&1) as u8) |
    ((((msr_low>>10)&1) as u8)<<1) |
    ((((msr_low>>8)&1) as u8)<<2)
}
/// Physical xAPIC register window from IA32_APIC_BASE (bits 12..35).
/// This is a discovery-only value: it MUST be mapped as uncacheable MMIO
/// before any dereference. x2APIC instead uses MSRs, not this window.
pub const fn xapic_physical_base(msr:u64)->Option<u64>{
    if msr&(1<<11)==0 || msr&(1<<10)!=0{return None;}
    let base=msr&0x0000_000f_ffff_f000;
    if base==0{return None;}
    Some(base)
}
/// Read the complete IA32_APIC_BASE MSR without changing APIC routing.
#[no_mangle]
pub extern "C" fn x86_64_timer_xapic_physical_base()->u64{
    let lo:u32;
    let hi:u32;
    unsafe{asm!("rdmsr",in("ecx")0x1bu32,out("eax")lo,out("edx")hi,
                options(nostack,preserves_flags));}
    xapic_physical_base(((hi as u64)<<32)|(lo as u64)).unwrap_or(0)
}
/// Returns firmware APIC mode captured immediately before PIC initialization.
/// 0xff means the experimental timer has not attempted initialization.
#[no_mangle]
pub extern "C" fn x86_64_timer_initial_apic_mode()->u8{
    INITIAL_APIC_MODE.load(Ordering::Relaxed)
}
#[no_mangle]
pub extern "C" fn x86_64_timer_apic_mode_flags()->u8{
    let low:u32;
    let high:u32;
    unsafe{asm!("rdmsr",in("ecx")0x1bu32,out("eax")low,out("edx")high,
                options(nostack,preserves_flags));}
    let _=high;
    apic_mode_flags(low)
}
#[no_mangle]
pub extern "C" fn x86_64_timer_pic_mask()->u8{
    if CONFIGURED.load(Ordering::Relaxed){inb(PIC1_DATA)}else{0xff}
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn irq0_diagnostics_distinguish_pending_in_service_and_masked(){
        assert_eq!(irq0_state(0,0,0),0);
        assert_eq!(irq0_state(1,0,0),1);
        assert_eq!(irq0_state(0,1,0),2);
        assert_eq!(irq0_state(1,1,1),7);
        assert_eq!(irq0_state(2,2,2),0);
    }
    #[test]
    fn apic_mode_decoding(){
        assert_eq!(apic_mode_flags(0),0);
        assert_eq!(apic_mode_flags(1<<11),1);
        assert_eq!(apic_mode_flags(1<<10),2);
        assert_eq!(apic_mode_flags(1<<8),4);
        assert_eq!(apic_mode_flags((1<<11)|(1<<10)|(1<<8)),7);
    }
    #[test]
    fn xapic_base_requires_enabled_xapic_mode(){
        assert_eq!(xapic_physical_base(0xfee0_0000),None);
        assert_eq!(xapic_physical_base(0xfee0_0000|(1<<11)),Some(0xfee0_0000));
        assert_eq!(xapic_physical_base(0xfee0_0000|(1<<11)|(1<<10)),None);
        assert_eq!(xapic_physical_base(1<<11),None);
        assert_eq!(xapic_physical_base(0x1_fee0_0000|(1<<11)),Some(0x1_fee0_0000));
    }
    #[test]
    fn counter_is_bounded(){
        assert_eq!(TICKS.load(Ordering::Relaxed),0);
        assert!(!CONFIGURED.load(Ordering::Relaxed));
    }
}
