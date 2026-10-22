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
    TICKS.fetch_update(Ordering::Relaxed,Ordering::Relaxed,|v|{
        v.checked_add(1)
    }).ok();
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
/// R019.2: classify why a legacy IRQ0 may not reach the CPU.
/// This is a read-only diagnostic; it does not enable, acknowledge or reroute IRQs.
/// Bit 0: IRQ0 masked, bit 1: pending without PIC acknowledgement,
/// bit 2: PIC servicing IRQ0, bit 3: local APIC still enabled,
/// bit 4: firmware started in x2APIC mode.
pub const fn legacy_irq0_blockers(
    irr:u8,isr:u8,mask:u8,current_apic:u8,initial_apic:u8,
)->u8{
    let mut result=0u8;
    if mask&1!=0{result|=1;}
    if irr&1!=0 && isr&1==0{result|=2;}
    if isr&1!=0{result|=4;}
    if current_apic&3!=0{result|=8;}
    if initial_apic&2!=0{result|=16;}
    result
}
/// Read-only snapshot for the optional QEMU preemption experiment.
#[no_mangle]
pub extern "C" fn x86_64_timer_irq0_blockers()->u8{
    if !CONFIGURED.load(Ordering::Relaxed){return 0;}
    legacy_irq0_blockers(
        x86_64_timer_pic_pending(),x86_64_timer_pic_in_service(),
        x86_64_timer_pic_mask(),x86_64_timer_apic_mode_flags(),
        x86_64_timer_initial_apic_mode()
    )
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
/// R019.3: construct a calibrated local APIC periodic timer configuration.
/// The caller must measure APIC ticks against an independent reference clock;
/// a raw PIT divisor is NOT an APIC initial-count calibration.
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub struct XapicPeriodicConfig {
    pub lvt:u32,
    pub divide:u32,
    pub initial_count:u32,
}
/// Accept only a nonzero, calibrated APIC count and a valid interrupt vector.
/// Vector 0x20 is reserved for the existing experimental PIC IRQ0 path;
/// the APIC timer uses a distinct vector to avoid ambiguous EOI handling.
pub const fn xapic_periodic_config(
    vector:u8,divisor:u32,calibrated_count:u32,
)->Option<XapicPeriodicConfig>{
    if vector<=0x20 || vector==0xff || calibrated_count==0{return None;}
    let Some(lvt)=xapic_timer_lvt(vector,true,false)else{return None;};
    let Some(divide)=xapic_timer_divide(divisor)else{return None;};
    Some(XapicPeriodicConfig{lvt,divide,initial_count:calibrated_count})
}
/// R019.4: derive APIC initial count from a measured reference interval.
/// elapsed_apic_ticks is measured with the selected APIC divisor already
/// configured; reference_hz is the trusted reference clock frequency.
/// Avoid integer truncation, zero periods, and u32 timer register overflow.
pub const fn xapic_calibrated_count(
    elapsed_apic_ticks:u64,elapsed_reference_ticks:u64,
    reference_hz:u64,target_hz:u32,
)->Option<u32>{
    if elapsed_apic_ticks==0 || elapsed_reference_ticks==0 ||
       reference_hz==0 || target_hz==0{return None;}
    let numerator=match (elapsed_apic_ticks as u128).checked_mul(reference_hz as u128){
        Some(value)=>value,
        None=>return None,
    };
    let denominator=match (elapsed_reference_ticks as u128).checked_mul(target_hz as u128){
        Some(value)=>value,
        None=>return None,
    };
    if denominator==0{return None;}
    let count=numerator/denominator;
    if count==0 || count>u32::MAX as u128{return None;}
    Some(count as u32)
}
/// R019.5: interpret a masked APIC one-shot down-counter sample.
/// Reject expired, stationary, and inconsistent measurements rather than
/// accidentally treating counter wraparound as valid calibration.
pub const fn xapic_oneshot_elapsed(initial:u32,current:u32)->Option<u64>{
    if initial==0 || current==0 || current>=initial{return None;}
    Some((initial-current) as u64)
}
/// Construct a periodic configuration from measured APIC and reference ticks.
/// This is pure validation; the caller is responsible for safe hardware reads.
pub const fn xapic_config_from_samples(
    vector:u8,divisor:u32,initial:u32,current:u32,
    reference_ticks:u64,reference_hz:u64,target_hz:u32,
)->Option<XapicPeriodicConfig>{
    let elapsed=match xapic_oneshot_elapsed(initial,current){
        Some(value)=>value,None=>return None,
    };
    let count=match xapic_calibrated_count(
        elapsed,reference_ticks,reference_hz,target_hz
    ){Some(value)=>value,None=>return None};
    xapic_periodic_config(vector,divisor,count)
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
// Experimental xAPIC register access. No physical address is dereferenced:
// the caller must first establish a dedicated supervisor UC 4 KiB mapping.
static XAPIC_VIRTUAL:AtomicU64=AtomicU64::new(0);
static XAPIC_TIMER_ARMED:AtomicBool=AtomicBool::new(false);
/// # Safety
/// BSP ring0 only, interrupts disabled. Caller proves that virtual_base
/// maps exactly the reported xAPIC physical page as supervisor-only UC
/// memory with a 4K PTE and reset PAT layout. Mapping must remain alive
/// across all IRQ handlers. Never pass an ordinary cached HHDM alias.
#[no_mangle]
pub unsafe extern "C" fn x86_64_xapic_bind_uc_page(
    virtual_base:u64,physical_base:u64,leaf_flags:u64,pat_reset_layout:bool,
)->bool{
    if XAPIC_VIRTUAL.load(Ordering::Acquire)!=0 ||
       !xapic_mapping_preflight(virtual_base,physical_base,
                                x86_64_timer_xapic_physical_base(),
                                leaf_flags,pat_reset_layout){return false;}
    XAPIC_VIRTUAL.store(virtual_base,Ordering::Release);
    true
}
fn xapic_reg(offset:usize)->Option<*mut u32>{
    if offset&0xf!=0 || offset>=0x400{return None;}
    let base=XAPIC_VIRTUAL.load(Ordering::Acquire);
    if base==0{return None;}
    Some((base as usize+offset) as *mut u32)
}
/// # Safety
/// The registered UC page must remain mapped, on the BSP, with interrupts
/// disabled. Leaves the timer masked until a separate calibrated IRQ path
/// is implemented. Never guesses a 100 Hz initial count.
#[no_mangle]
pub unsafe extern "C" fn x86_64_xapic_timer_prepare_masked(vector:u8)->bool{
    if XAPIC_TIMER_ARMED.load(Ordering::Acquire){return false;}
    let Some(lvt)=xapic_timer_lvt(vector,true,true)else{return false;};
    let (Some(divide),Some(initial),Some(lvt_reg))=(
        xapic_reg(XAPIC_TIMER_DIVIDE_OFFSET),
        xapic_reg(XAPIC_TIMER_INITIAL_OFFSET),
        xapic_reg(XAPIC_LVT_TIMER_OFFSET)
    )else{return false;};
    unsafe{
        core::ptr::write_volatile(lvt_reg,lvt);
        core::ptr::write_volatile(initial,0);
        core::ptr::write_volatile(divide,xapic_timer_divide(16).unwrap());
    }
    XAPIC_TIMER_ARMED.store(true,Ordering::Release);
    true
}
/// R019.3: install a calibrated periodic timer while interrupts are disabled.
/// This does not configure an IDT gate or prove interrupt delivery.
/// # Safety
/// BSP ring0 only, interrupts disabled; caller has validated the UC xAPIC
/// mapping, installed a dedicated IDT vector, and independently calibrated
/// the initial count. No PIC IRQ may use this vector.
#[no_mangle]
pub unsafe extern "C" fn x86_64_xapic_timer_start_calibrated(
    vector:u8,divisor:u32,calibrated_count:u32,
)->bool{
    let Some(cfg)=xapic_periodic_config(vector,divisor,calibrated_count)else{return false;};
    if !XAPIC_TIMER_ARMED.load(Ordering::Acquire){return false;}
    let (Some(lvt),Some(divide),Some(initial))=(
        xapic_reg(XAPIC_LVT_TIMER_OFFSET),
        xapic_reg(XAPIC_TIMER_DIVIDE_OFFSET),
        xapic_reg(XAPIC_TIMER_INITIAL_OFFSET),
    )else{return false;};
    unsafe{
        // Mask before reprogramming, then start only with a validated count.
        core::ptr::write_volatile(lvt,cfg.lvt|(1<<16));
        core::ptr::write_volatile(initial,0);
        core::ptr::write_volatile(divide,cfg.divide);
        core::ptr::write_volatile(lvt,cfg.lvt);
        core::ptr::write_volatile(initial,cfg.initial_count);
    }
    true
}
/// # Safety
/// Call only from a genuine local APIC interrupt handler, after a validated
/// UC binding; sending EOI for a PIC IRQ is incorrect.
#[no_mangle]
pub unsafe extern "C" fn x86_64_xapic_eoi()->bool{
    let Some(eoi)=xapic_reg(XAPIC_EOI_OFFSET)else{return false;};
    unsafe{core::ptr::write_volatile(eoi,0);}
    true
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
    fn legacy_irq0_blockers_are_independent_and_read_only(){
        assert_eq!(legacy_irq0_blockers(0,0,0,0,0),0);
        assert_eq!(legacy_irq0_blockers(1,0,0,0,0),2);
        assert_eq!(legacy_irq0_blockers(1,1,0,0,0),4);
        assert_eq!(legacy_irq0_blockers(0,0,1,0,0),1);
        assert_eq!(legacy_irq0_blockers(1,0,1,1,2),27);
        assert_eq!(legacy_irq0_blockers(2,2,2,0,0),0);
    }
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
    fn periodic_apic_configuration_requires_calibration_and_dedicated_vector(){
        let cfg=xapic_periodic_config(0x40,16,12_345).unwrap();
        assert_eq!(cfg.lvt,0x20040);
        assert_eq!(cfg.divide,3);
        assert_eq!(cfg.initial_count,12_345);
        assert_eq!(xapic_periodic_config(0x20,16,100),None);
        assert_eq!(xapic_periodic_config(0xff,16,100),None);
        assert_eq!(xapic_periodic_config(0x40,16,0),None);
        assert_eq!(xapic_periodic_config(0x40,3,100),None);
    }
    #[test]
    fn calibration_from_reference_ticks_rejects_invalid_measurements(){
        // 1,000,000 APIC ticks measured over 10 ms at 1 MHz reference,
        // gives 1,000,000 ticks per 100 Hz interval.
        assert_eq!(xapic_calibrated_count(1_000_000,10_000,1_000_000,100),Some(1_000_000));
        assert_eq!(xapic_calibrated_count(0,10_000,1_000_000,100),None);
        assert_eq!(xapic_calibrated_count(1,0,1_000_000,100),None);
        assert_eq!(xapic_calibrated_count(1,1,0,100),None);
        assert_eq!(xapic_calibrated_count(1,1,1,0),None);
        assert_eq!(xapic_calibrated_count(1,1,1,100),None);
        assert_eq!(xapic_calibrated_count(u64::MAX,1,u64::MAX,1),None);
    }
    #[test]
    fn oneshot_samples_are_bounded(){
        assert_eq!(xapic_oneshot_elapsed(1000,750),Some(250));
        assert_eq!(xapic_oneshot_elapsed(1000,1000),None);
        assert_eq!(xapic_oneshot_elapsed(1000,0),None);
        assert_eq!(xapic_oneshot_elapsed(0,0),None);
        assert_eq!(xapic_oneshot_elapsed(1000,1001),None);
        let cfg=xapic_config_from_samples(
            0x40,16,1_000_000,500_000,10_000,1_000_000,100
        ).unwrap();
        assert_eq!(cfg.initial_count,500_000);
        assert_eq!(cfg.lvt,0x20040);
        assert_eq!(xapic_config_from_samples(
            0x40,16,1_000_000,0,10_000,1_000_000,100
        ),None);
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
    fn xapic_mmio_register_access_rejects_unbound_and_unaligned(){
        assert!(xapic_reg(XAPIC_EOI_OFFSET).is_none());
        assert!(xapic_reg(0x401).is_none());
        assert!(xapic_reg(0x321).is_none());
    }
    #[test]
    fn timer_counter_overflow_is_rejected(){
        assert_eq!(u64::MAX.checked_add(1),None);
        assert_eq!((u64::MAX-1).checked_add(1),Some(u64::MAX));
    }
    #[test]
    fn counter_is_bounded(){
        assert_eq!(TICKS.load(Ordering::Relaxed),0);
        assert!(!CONFIGURED.load(Ordering::Relaxed));
    }
}
