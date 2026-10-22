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
/// xAPIC timer MMIO offsets, relative to a separately mapped UC page.
/// This module deliberately does not dereference physical addresses.
pub const XAPIC_EOI_OFFSET:usize=0x0b0;
pub const XAPIC_SPURIOUS_OFFSET:usize=0x0f0;
pub const XAPIC_LVT_TIMER_OFFSET:usize=0x320;
pub const XAPIC_TIMER_INITIAL_OFFSET:usize=0x380;
pub const XAPIC_TIMER_CURRENT_OFFSET:usize=0x390;
pub const XAPIC_TIMER_DIVIDE_OFFSET:usize=0x3e0;
/// Validate the kernel-supplied mapping before any future volatile MMIO.
/// The caller must additionally establish page-table UC/PAT attributes,
/// supervisor-only permissions, and a physical mapping to IA32_APIC_BASE.
pub const fn xapic_register_page_valid(base:usize)->bool{
    base!=0 && base&0xfff==0 && base.checked_add(0x400).is_some()
}
/// Local APIC timer LVT: vector 0x20..0xfe, periodic bit 17, mask bit 16.
/// Reject vectors reserved for exceptions, spurious interrupts or invalid
/// configurations. The returned value is a configuration word, not an MMIO write.
pub const fn xapic_timer_lvt(vector:u8,periodic:bool,masked:bool)->Option<u32>{
    if vector<0x20 || vector==0xff{return None;}
    Some((vector as u32)|((periodic as u32)<<17)|((masked as u32)<<16))
}
/// Divide configuration encoding from Intel SDM: 1,2,4,8,16,32,64,128.
pub const fn xapic_timer_divide(divisor:u32)->Option<u32>{
    match divisor{
        1=>Some(0x0b),2=>Some(0),4=>Some(1),8=>Some(2),
        16=>Some(3),32=>Some(8),64=>Some(9),128=>Some(0x0a),
        _=>None,
    }
}
/// Verify a pre-existing supervisor-only, uncacheable 4 KiB xAPIC
/// mapping. The caller must first resolve the virtual page through the
/// current page tables and supply the translated physical base and leaf PTE
/// flags. PCD+PWT is the conservative UC encoding only when PAT is in its
/// architectural reset configuration; a future mapper must inspect PAT MSR.
pub const fn xapic_mapping_preflight(
    virtual_base:u64,translated_physical:u64,expected_physical:u64,
    leaf_flags:u64,pat_reset_layout:bool,
)->bool{
    const PRESENT:u64=1;
    const WRITABLE:u64=1<<1;
    const USER:u64=1<<2;
    const PWT:u64=1<<3;
    const PCD:u64=1<<4;
    const HUGE:u64=1<<7;
    const PAT_4K:u64=1<<7;
    if !pat_reset_layout || virtual_base==0 || virtual_base&0xfff!=0 ||
       virtual_base>>48!=0xffff || virtual_base&(1<<47)==0 ||
       expected_physical==0 || expected_physical&0xfff!=0 ||
       translated_physical!=expected_physical{return false;}
    // This preflight accepts a 4K leaf only, never a large-page mapping.
    // At PTE level bit 7 is PAT, which must be clear for UC index 3.
    let required=PRESENT|WRITABLE|PWT|PCD;
    leaf_flags&required==required && leaf_flags&(USER|PAT_4K)==0
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
    fn xapic_register_layout_and_mapping_preflight(){
        assert_eq!(XAPIC_EOI_OFFSET,0xb0);
        assert_eq!(XAPIC_LVT_TIMER_OFFSET,0x320);
        assert_eq!(XAPIC_TIMER_INITIAL_OFFSET,0x380);
        assert_eq!(XAPIC_TIMER_CURRENT_OFFSET,0x390);
        assert_eq!(XAPIC_TIMER_DIVIDE_OFFSET,0x3e0);
        assert!(xapic_register_page_valid(0xffff_8000_0000_0000));
        assert!(!xapic_register_page_valid(0));
        assert!(!xapic_register_page_valid(0x1234));
        assert!(!xapic_register_page_valid(usize::MAX-0xff));
    }
    #[test]
    fn xapic_timer_configuration_validation(){
        assert_eq!(xapic_timer_lvt(0x20,true,false),Some(0x20020));
        assert_eq!(xapic_timer_lvt(0x40,false,true),Some(0x10040));
        assert_eq!(xapic_timer_lvt(0x1f,true,false),None);
        assert_eq!(xapic_timer_lvt(0xff,true,false),None);
        for (d,v) in [(1,0xb),(2,0),(4,1),(8,2),(16,3),(32,8),(64,9),(128,0xa)]{
            assert_eq!(xapic_timer_divide(d),Some(v));
        }
        assert_eq!(xapic_timer_divide(0),None);
        assert_eq!(xapic_timer_divide(3),None);
    }
    #[test]
    fn xapic_mapping_requires_supervisor_uc_leaf(){
        let va=0xffff_8000_fee0_0000u64;
        let pa=0xfee0_0000u64;
        let flags=1|2|8|16;
        assert!(xapic_mapping_preflight(va,pa,pa,flags,true));
        assert!(!xapic_mapping_preflight(va,pa,pa,flags,false));
        assert!(!xapic_mapping_preflight(va,pa,pa,flags|4,true));
        assert!(!xapic_mapping_preflight(va,pa,pa,flags|128,true));
        assert!(!xapic_mapping_preflight(va,pa,pa,flags&!16,true));
        assert!(!xapic_mapping_preflight(va+1,pa,pa,flags,true));
        assert!(!xapic_mapping_preflight(va,pa+4096,pa,flags,true));
        assert!(!xapic_mapping_preflight(0xfee0_0000,pa,pa,flags,true));
    }
    #[test]
    fn counter_is_bounded(){
        assert_eq!(TICKS.load(Ordering::Relaxed),0);
        assert!(!CONFIGURED.load(Ordering::Relaxed));
    }
}
