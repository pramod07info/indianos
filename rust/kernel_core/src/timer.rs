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
// Counts successfully acknowledged local APIC interrupts separately from PIC IRQ0.
static XAPIC_IRQ_TICKS:AtomicU64=AtomicU64::new(0);
/// Cumulative successfully delivered BSP timer interrupts. Unlike the
/// diagnostic per-session counters, this NEVER resets on calibration or
/// stop/restart; DHCP receipt timestamps must not roll backwards.
static MONOTONIC_IRQ_TICKS:AtomicU64=AtomicU64::new(0);
/// 0: no authoritative source, 1: programmed PIT/PIC, 2: calibrated xAPIC.
static MONOTONIC_SOURCE:AtomicU8=AtomicU8::new(0);
/// A restarted timer must deliver a NEW IRQ before its clock can be trusted.
static MONOTONIC_SESSION_PROGRESS:AtomicBool=AtomicBool::new(false);
/// A saturated u64 must never wrap back to an old lease timestamp.
static MONOTONIC_EXHAUSTED:AtomicBool=AtomicBool::new(false);
/// Session changes invalidate concurrent time samples, including same-source
/// stop/restart (source ID alone is vulnerable to an ABA race).
static MONOTONIC_SESSION_EPOCH:AtomicU64=AtomicU64::new(0);
/// Only an IRQ stamped with the *current* timer epoch may authorize time.
static MONOTONIC_PROGRESS_EPOCH:AtomicU64=AtomicU64::new(0);
fn irq_receipt_valid(epoch:u64,receipt_epoch:u64,progress:bool)->bool{
    epoch!=0 && epoch==receipt_epoch && progress
}
fn invalidate_monotonic_session(){
    if MONOTONIC_SESSION_EPOCH.fetch_update(
        Ordering::AcqRel,Ordering::Acquire,|n|n.checked_add(1)
    ).is_err(){
        MONOTONIC_EXHAUSTED.store(true,Ordering::Release);
    }
    MONOTONIC_SESSION_PROGRESS.store(false,Ordering::Release);
    MONOTONIC_PROGRESS_EPOCH.store(0,Ordering::Release);
}
fn same_timer_session(start:u64,end:u64,source_before:u8,source_after:u8)->bool{
    start==end&&source_before==source_after
}
// Counts actual valid ring-3 context captures, not merely APIC EOI writes.
static XAPIC_PREEMPTIONS:AtomicU64=AtomicU64::new(0);
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
/// Isolated pre-user QEMU NIC profile only. Reuses legacy PIC/APIC routing;
/// every source stays masked until its one-shot IRQ11 notification window.
#[cfg(feature="test-r136-nic-irq")]
pub(crate) unsafe fn prepare_exclusive_nic_pic()->bool{
 if !select_legacy_pic(){return false;}
 outb(PIC1_DATA,0xff);outb(PIC2_DATA,0xff);
 outb(PIC1_CMD,0x11);io_wait();outb(PIC2_CMD,0x11);io_wait();
 outb(PIC1_DATA,0x20);io_wait();outb(PIC2_DATA,0x28);io_wait();
 outb(PIC1_DATA,0x04);io_wait();outb(PIC2_DATA,0x02);io_wait();
 outb(PIC1_DATA,0x01);io_wait();outb(PIC2_DATA,0x01);io_wait();
 outb(PIC1_DATA,0xfb);outb(PIC2_DATA,0xff);true
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
    CONFIGURED.store(true,Ordering::Release);
    // Prefer an actively calibrated APIC, when one exists. Never let an
    // uncalibrated second source silently produce double-counted time.
    if !XAPIC_TIMER_RUNNING.load(Ordering::Acquire) {
        invalidate_monotonic_session();
        MONOTONIC_SOURCE.store(1,Ordering::Release);
    }else{
        MONOTONIC_SOURCE.store(0,Ordering::Release);
        invalidate_monotonic_session();
    }
    true
}
#[no_mangle]
pub extern "C" fn x86_64_timer_ack_irq0(){
    if !CONFIGURED.load(Ordering::Relaxed){return}
    TICKS.fetch_update(Ordering::Relaxed,Ordering::Relaxed,|v|{
        v.checked_add(1)
    }).ok();
    record_kernel_monotonic_tick(1);
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
    if vector!=0x40 || calibrated_count==0{return None;}
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
/// R019.12: bound a prospective APIC timer calibration window before
/// programming hardware. A 32-bit down-counter must not wrap during the
/// measurement, and a reference interval must be strictly positive.
pub const fn xapic_calibration_window_valid(
    initial_count:u32, current_count:u32, reference_ticks:u64,
)->bool{
    initial_count>0 && current_count>0 &&
    current_count<initial_count && reference_ticks>0
}
/// R019.13: validate a hardware calibration sample and derive the periodic
/// configuration as one fail-closed operation. The APIC counter is 32-bit,
/// so a stopped/wrapped sample must never arm the timer.
pub const fn xapic_checked_calibration(
    initial:u32,current:u32,reference_ticks:u64,reference_hz:u64,
    target_hz:u32,
)->Option<XapicPeriodicConfig>{
    if !xapic_calibration_window_valid(initial,current,reference_ticks){
        return None;
    }
    xapic_config_from_samples(
        0x40,16,initial,current,reference_ticks,reference_hz,target_hz
    )
}
/// R019.13: ensure the periodic count cannot exceed the measured initial
/// one-shot count when the requested period is no longer than the sample
/// interval. This check is for test/diagnostic use, not hardware setup.
pub const fn xapic_periodic_sample_consistent(
    cfg:XapicPeriodicConfig,initial:u32,current:u32,
    reference_ticks:u64,reference_hz:u64,target_hz:u32,
)->bool{
    let Some(expected)=xapic_checked_calibration(
        initial,current,reference_ticks,reference_hz,target_hz
    )else{return false;};
    cfg.initial_count==expected.initial_count &&
    cfg.divide==expected.divide && cfg.lvt==expected.lvt
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
// Unlike ARMED (UC binding/initialization), RUNNING denotes an unmasked
// periodic timer. A calibration one-shot must never trigger preemption.
static XAPIC_TIMER_RUNNING:AtomicBool=AtomicBool::new(false);
// A masked register alone does not establish an active measurement.
static XAPIC_CALIBRATING:AtomicBool=AtomicBool::new(false);
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
    if vector!=0x40 || XAPIC_TIMER_ARMED.load(Ordering::Acquire){return false;}
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
    // Never reprogram a live periodic timer. Stop/mask it first so stale
    // IRQs cannot be counted as progress for the next calibration session.
    if !XAPIC_TIMER_ARMED.load(Ordering::Acquire) ||
       XAPIC_TIMER_RUNNING.load(Ordering::Acquire){return false;}
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
        // A fresh timer session must not inherit old IRQ/preemption evidence.
        // BSP interrupts are disabled throughout this critical section.
        XAPIC_IRQ_TICKS.store(0,Ordering::Release);
        XAPIC_PREEMPTIONS.store(0,Ordering::Release);
        core::ptr::write_volatile(lvt,cfg.lvt);
        core::ptr::write_volatile(initial,cfg.initial_count);
    }
    XAPIC_CALIBRATING.store(false,Ordering::Release);
    XAPIC_TIMER_RUNNING.store(true,Ordering::Release);
    invalidate_monotonic_session();
    MONOTONIC_SOURCE.store(2,Ordering::Release);
    true
}
/// R019.8: begin a bounded masked one-shot measurement.
/// A separate reference clock must measure the interval; the timer remains
/// masked throughout, so this cannot cause an unhandled APIC interrupt.
/// Rejects a second begin until stop or periodic start ends the measurement;
/// restarting the counter would invalidate the outstanding reference interval.
/// # Safety
/// BSP ring0 only with interrupts disabled, valid UC xAPIC mapping,
/// and no concurrent timer user. Caller must stop sampling before wrap.
#[no_mangle]
pub unsafe extern "C" fn x86_64_xapic_calibration_begin(initial_count:u32)->bool{
    if XAPIC_TIMER_RUNNING.load(Ordering::Acquire) ||
       XAPIC_CALIBRATING.load(Ordering::Acquire){return false;}
    if initial_count==0 || !XAPIC_TIMER_ARMED.load(Ordering::Acquire){return false;}
    let (Some(lvt),Some(divide),Some(initial))=(
        xapic_reg(XAPIC_LVT_TIMER_OFFSET),
        xapic_reg(XAPIC_TIMER_DIVIDE_OFFSET),
        xapic_reg(XAPIC_TIMER_INITIAL_OFFSET)
    )else{return false;};
    let Some(lvt_value)=xapic_timer_lvt(0x40,false,true)else{return false;};
    let Some(divide_value)=xapic_timer_divide(16)else{return false;};
    unsafe{
        core::ptr::write_volatile(lvt,lvt_value);
        core::ptr::write_volatile(initial,0);
        core::ptr::write_volatile(divide,divide_value);
        core::ptr::write_volatile(initial,initial_count);
    }
    XAPIC_CALIBRATING.store(true,Ordering::Release);
    true
}
/// R019.7: sample the current count of a prepared, masked xAPIC timer.
/// Returns zero outside an active masked calibration; a zero count is not
/// usable for calibration. Does not start or acknowledge any interrupt.
/// # Safety
/// Ring0 BSP only, with interrupts disabled and the UC mapping alive.
#[no_mangle]
pub unsafe extern "C" fn x86_64_xapic_timer_sample_masked()->u32{
    if !XAPIC_TIMER_ARMED.load(Ordering::Acquire) ||
       !XAPIC_CALIBRATING.load(Ordering::Acquire){return 0;}
    let Some(lvt)=xapic_reg(XAPIC_LVT_TIMER_OFFSET)else{return 0;};
    let Some(current)=xapic_reg(XAPIC_TIMER_CURRENT_OFFSET)else{return 0;};
    if unsafe{core::ptr::read_volatile(lvt)}&(1<<16)==0{return 0;}
    unsafe{core::ptr::read_volatile(current)}
}
/// R019.6: stop the experimental APIC timer without touching PIC IRQ0.
/// # Safety
/// BSP ring0 only, interrupts disabled, with the validated UC APIC page
/// still mapped. This is an explicit quiesce operation, not a scheduler.
#[no_mangle]
pub unsafe extern "C" fn x86_64_xapic_timer_stop()->bool{
    if !XAPIC_TIMER_ARMED.load(Ordering::Acquire){return false;}
    let (Some(lvt),Some(initial))=(
        xapic_reg(XAPIC_LVT_TIMER_OFFSET),
        xapic_reg(XAPIC_TIMER_INITIAL_OFFSET),
    )else{return false;};
    unsafe{
        let old=core::ptr::read_volatile(lvt);
        core::ptr::write_volatile(lvt,old|(1<<16));
        core::ptr::write_volatile(initial,0);
    }
    XAPIC_CALIBRATING.store(false,Ordering::Release);
    XAPIC_TIMER_RUNNING.store(false,Ordering::Release);
    if MONOTONIC_SOURCE.compare_exchange(
        2,0,Ordering::AcqRel,Ordering::Acquire).is_ok(){
        invalidate_monotonic_session();
    }
    true
}
/// Quiesce every timer source owned by this single-BSP experimental
/// kernel before returning to the read-only recovery console. The caller
/// MUST have disabled CPU interrupts (CLI) and must not subsequently enable
/// them during shutdown. A failed MMIO or PIC readback prevents recovery
/// services from starting. This intentionally leaves unrelated IRQ lines and
/// firmware-owned devices untouched.
/// # Safety
/// BSP ring0, interrupts disabled; any registered UC xAPIC mapping must
/// still be valid. This routine never unmasks or starts timer hardware.
/// Run both independent quiescence operations even when one fails.
/// A failed APIC stop must not leave the legacy IRQ0 route unnecessarily live.
/// This helper is injected by host tests without touching privileged registers.
fn quiesce_owned_sources(
    xapic_armed:bool, legacy_armed:bool,
    stop_apic:impl FnOnce()->bool,
    mask_irq0:impl FnOnce()->bool,
)->bool {
    let apic_ok=if xapic_armed {stop_apic()} else {true};
    let pic_ok=if legacy_armed {mask_irq0()} else {true};
    apic_ok && pic_ok
}
pub unsafe fn quiesce_for_shutdown() -> bool {
    let safe=quiesce_owned_sources(
        XAPIC_TIMER_ARMED.load(Ordering::Acquire),
        CONFIGURED.load(Ordering::Acquire),
        || {
            if !unsafe { x86_64_xapic_timer_stop() } {return false;}
            let (Some(lvt),Some(initial))=(
                xapic_reg(XAPIC_LVT_TIMER_OFFSET),
                xapic_reg(XAPIC_TIMER_INITIAL_OFFSET)
            ) else {return false;};
            // Verify hardware registers rather than only software flags.
            let lvt_value:u32=unsafe {core::ptr::read_volatile(lvt)};
            let initial_count:u32=unsafe {core::ptr::read_volatile(initial)};
            (lvt_value & (1u32<<16))!=0 && initial_count==0
        },
        || {
            // PIT may continue counting; mask its kernel-owned PIC IRQ0.
            let new_mask=inb(PIC1_DATA)|1;
            outb(PIC1_DATA,new_mask);
            if inb(PIC1_DATA)&1==0 {return false;}
            CONFIGURED.store(false,Ordering::Release);
            if MONOTONIC_SOURCE.compare_exchange(
                1,0,Ordering::AcqRel,Ordering::Acquire).is_ok(){
                invalidate_monotonic_session();
            }
            true
        },
    );
    safe && !XAPIC_TIMER_RUNNING.load(Ordering::Acquire) &&
      !XAPIC_CALIBRATING.load(Ordering::Acquire) &&
      !CONFIGURED.load(Ordering::Acquire)
}
#[cfg(test)]
mod r114_shutdown_quiescence_tests {
    use super::quiesce_owned_sources;
    use std::cell::RefCell;
    #[test]
    fn shutdown_attempts_both_sources_even_after_apic_failure() {
        let history=RefCell::new(Vec::new());
        let ok=quiesce_owned_sources(true,true,
            ||{history.borrow_mut().push("apic");false},
            ||{history.borrow_mut().push("irq0");true});
        assert!(!ok);
        assert_eq!(*history.borrow(),["apic","irq0"]);
    }
    #[test]
    fn shutdown_refuses_each_independent_failed_readback() {
        assert!(!quiesce_owned_sources(true,true,||true,||false));
        assert!(!quiesce_owned_sources(true,true,||false,||true));
        assert!(!quiesce_owned_sources(true,true,||false,||false));
        assert!(quiesce_owned_sources(true,true,||true,||true));
    }
    #[test]
    fn only_initialized_sources_are_touched() {
        assert!(quiesce_owned_sources(false,false,
            ||panic!("unarmed APIC"),||panic!("unarmed PIC")));
        assert!(quiesce_owned_sources(true,false,
            ||true,||panic!("unarmed PIC")));
        assert!(quiesce_owned_sources(false,true,
            ||panic!("unarmed APIC"),||true));
    }
}

/// # Safety
/// Call only from a genuine local APIC interrupt handler, after a validated
/// UC binding; sending EOI for a PIC IRQ is incorrect.
#[no_mangle]
pub unsafe extern "C" fn x86_64_xapic_eoi()->bool{
    let Some(eoi)=xapic_reg(XAPIC_EOI_OFFSET)else{return false;};
    unsafe{core::ptr::write_volatile(eoi,0);}
    XAPIC_IRQ_TICKS.fetch_update(Ordering::Relaxed,Ordering::Relaxed,|v|v.checked_add(1)).ok();
    record_kernel_monotonic_tick(2);
    true
}
/// Read-only APIC IRQ delivery evidence, separate from legacy PIC IRQ0.
#[no_mangle]
pub extern "C" fn x86_64_xapic_timer_irq_count()->u64{
    XAPIC_IRQ_TICKS.load(Ordering::Relaxed)
}
/// Called by the actual configured PIC/APIC IRQ acknowledgement path only.
/// Ignore unrelated or stopped sources, and reject irreversible overflow.
fn current_irq_session(start_epoch:u64,end_epoch:u64,
    expected_source:u8,observed_source:u8)->bool{
    (expected_source==1||expected_source==2)&&
       same_timer_session(start_epoch,end_epoch,expected_source,observed_source)
}
/// Only an actually armed interrupt controller can authenticate a receipt.
/// The APIC wins over PIC if both remain configured during switchover.
fn source_irq_ready(source:u8,pic_armed:bool,apic_running:bool)->bool{
    (source==1 && pic_armed && !apic_running) ||
    (source==2 && apic_running)
}
fn tick_session_authorized(epoch:u64,current_epoch:u64,
    source:u8,current_source:u8,pic_armed:bool,apic_running:bool,
    exhausted:bool)->bool{
    epoch!=0 && !exhausted &&
    current_irq_session(epoch,current_epoch,source,current_source) &&
    source_irq_ready(source,pic_armed,apic_running)
}
fn record_kernel_monotonic_tick(source:u8){
    let epoch=MONOTONIC_SESSION_EPOCH.load(Ordering::Acquire);
    if !tick_session_authorized(epoch,
        MONOTONIC_SESSION_EPOCH.load(Ordering::Acquire),source,
        MONOTONIC_SOURCE.load(Ordering::Acquire),
        CONFIGURED.load(Ordering::Acquire),
        XAPIC_TIMER_RUNNING.load(Ordering::Acquire),
        MONOTONIC_EXHAUSTED.load(Ordering::Acquire)){return;}
    if MONOTONIC_IRQ_TICKS.fetch_update(
        Ordering::AcqRel,Ordering::Acquire,|n|n.checked_add(1)
    ).is_err(){
        MONOTONIC_EXHAUSTED.store(true,Ordering::Release);
    }else if tick_session_authorized(epoch,
        MONOTONIC_SESSION_EPOCH.load(Ordering::Acquire),source,
        MONOTONIC_SOURCE.load(Ordering::Acquire),
        CONFIGURED.load(Ordering::Acquire),
        XAPIC_TIMER_RUNNING.load(Ordering::Acquire),
        MONOTONIC_EXHAUSTED.load(Ordering::Acquire)){
        // Stamp first, then publish: a callback that lost a reset race
        // carries the OLD stamp and cannot authorize the new session.
        MONOTONIC_PROGRESS_EPOCH.store(epoch,Ordering::Release);
        MONOTONIC_SESSION_PROGRESS.store(true,Ordering::Release);
    }
}

/// Read a cumulative, delivered-IRQ clock across timer restarts. Its tick
/// count is monotonically increasing across PIC/APIC session calibration;
/// neither the PIC diagnostic count nor the APIC diagnostic count alone is
/// suitable to authorize a lease after a timer stop/restart.
/// This still is NOT a high-resolution cross-CPU clock and cannot account
/// for elapsed time while all timer sources are stopped.
pub fn kernel_monotonic_100hz() -> Option<u64> {
    let epoch=MONOTONIC_SESSION_EPOCH.load(Ordering::Acquire);
    let source=MONOTONIC_SOURCE.load(Ordering::Acquire);
    let receipt=MONOTONIC_PROGRESS_EPOCH.load(Ordering::Acquire);
    let progress=irq_receipt_valid(epoch,receipt,
        MONOTONIC_SESSION_PROGRESS.load(Ordering::Acquire));
    let sample=authoritative_monotonic_sample(
        source,
        progress,
        MONOTONIC_EXHAUSTED.load(Ordering::Acquire),
        MONOTONIC_IRQ_TICKS.load(Ordering::Acquire),
        XAPIC_TIMER_RUNNING.load(Ordering::Acquire),
        CONFIGURED.load(Ordering::Acquire),
    );
    let end_epoch=MONOTONIC_SESSION_EPOCH.load(Ordering::Acquire);
    let end_source=MONOTONIC_SOURCE.load(Ordering::Acquire);
    let end_receipt=MONOTONIC_PROGRESS_EPOCH.load(Ordering::Acquire);
    if !monotonic_final_authorized(epoch,end_epoch,source,end_source,
        receipt,end_receipt,
        MONOTONIC_SESSION_PROGRESS.load(Ordering::Acquire),
        MONOTONIC_EXHAUSTED.load(Ordering::Acquire),
        CONFIGURED.load(Ordering::Acquire),
        XAPIC_TIMER_RUNNING.load(Ordering::Acquire)){return None;}
    sample
}

// The final receipt prevents stop/rearm races from publishing stale time.
fn monotonic_snapshot_fenced(start:u64,end:u64,source:u8,end_source:u8,
    receipt:u64,end_receipt:u64,progress:bool,exhausted:bool)->bool{
    !exhausted && progress && same_timer_session(start,end,source,end_source) &&
    irq_receipt_valid(start,receipt,progress) &&
    irq_receipt_valid(end,end_receipt,progress)
}
/// Recheck timer-controller ownership after the final IRQ receipt; stopping a
/// PIC or promoting an APIC must not publish a stale PIC sample.
fn monotonic_final_authorized(start:u64,end:u64,source:u8,end_source:u8,
    receipt:u64,end_receipt:u64,progress:bool,exhausted:bool,
    pic_armed:bool,apic_running:bool)->bool {
    monotonic_snapshot_fenced(start,end,source,end_source,receipt,end_receipt,
        progress,exhausted) &&
    source_irq_ready(source,pic_armed,apic_running)
}
/// Pure publication guard: no new IRQ in the current session, a stopped
/// source, conflicting APIC/PIC authority or numeric overflow deny time.
pub const fn authoritative_monotonic_sample(
    source:u8,progress:bool,exhausted:bool,ticks:u64,
    apic_running:bool,pic_running:bool,
)->Option<u64>{
    if !progress||exhausted||ticks==0{return None;}
    if (source==2&&apic_running)||
       (source==1&&pic_running&&!apic_running){
        Some(ticks)
    }else{None}
}

/// Pure availability gate; 0 delivered IRQs must never authenticate an
/// interval just because a timer was configured. xAPIC wins when active.
pub const fn calibrated_kernel_ticks(
    xapic_running:bool,pic_running:bool,xapic_ticks:u64,pic_ticks:u64
)->Option<u64>{
    if xapic_running {
        if xapic_ticks>0 {Some(xapic_ticks)} else {None}
    }else if pic_running {
        if pic_ticks>0 {Some(pic_ticks)} else {None}
    }else{None}
}
#[cfg(test)]
mod r118_monotonic_hardware_tick_tests{
    use super::calibrated_kernel_ticks;
    #[test]fn unconfigured_or_missing_irq_progress_has_no_clock_authority(){
        assert_eq!(calibrated_kernel_ticks(false,false,99,99),None);
        assert_eq!(calibrated_kernel_ticks(true,false,0,99),None);
        assert_eq!(calibrated_kernel_ticks(false,true,99,0),None);
        assert_eq!(calibrated_kernel_ticks(true,false,8,99),Some(8));
        assert_eq!(calibrated_kernel_ticks(false,true,0,13),Some(13));
    }
    #[test]fn calibrated_apic_precedence_and_saturated_counter_are_preserved(){
        assert_eq!(calibrated_kernel_ticks(true,true,7,1000),Some(7));
        assert_eq!(calibrated_kernel_ticks(true,true,0,1000),None);
        assert_eq!(calibrated_kernel_ticks(false,true,0,u64::MAX),
                   Some(u64::MAX));
    }
}

/// Count successful ring-3 APIC preemptions independently of delivered IRQs.
/// Called only after a validated user frame has been captured.
fn preemption_capture_budget(delivered:u64,captured:u64,running:bool)->bool {
    running && captured<delivered
}
// Session fencing denies capture when a timer was stopped and rearmed,
// even when its source ID remains the same.
fn preemption_session_valid(before:u64,after:u64,
    delivered:u64,captured:u64,running:bool)->bool {
    before!=0 && before!=u64::MAX && before==after &&
    preemption_capture_budget(delivered,captured,running)
}
// Reject IRQ count rewind while a live user preemption is published.
fn preemption_delivery_receipt_valid(a:u64,b:u64,first:u64,last:u64,
    captured:u64,running:bool)->bool{
    first!=u64::MAX && last!=u64::MAX && last>=first &&
    preemption_session_valid(a,b,first,captured,running)
}
// A PIC takeover or timer-source revocation cannot mint APIC preemption
// evidence from stale but still-readable hardware IRQ counters.
fn apic_preemption_receipt_valid(before:u64,after:u64,first:u64,last:u64,
    captured:u64,running:bool,source_before:u8,source_after:u8)->bool {
    source_before==2 && source_after==2 &&
    preemption_delivery_receipt_valid(before,after,first,last,captured,running)
}
fn apic_preemption_receipt_authorized(
    before_epoch:u64,after_epoch:u64,first_irq:u64,last_irq:u64,
    captured:u64,running:bool,source_before:u8,source_after:u8,
    exhausted_before:bool,exhausted_after:bool
)->bool {
    !exhausted_before && !exhausted_after &&
    apic_preemption_receipt_valid(before_epoch,after_epoch,first_irq,
        last_irq,captured,running,source_before,source_after)
}
pub fn xapic_record_preemption(){
    // Carry both exhaustion observations across the IRQ accounting
    // transaction, not only the publication side.
    let epoch=MONOTONIC_SESSION_EPOCH.load(Ordering::Acquire);
    let source=MONOTONIC_SOURCE.load(Ordering::Acquire);
    let exhausted=MONOTONIC_EXHAUSTED.load(Ordering::Acquire);
    let delivered=XAPIC_IRQ_TICKS.load(Ordering::Acquire);
    XAPIC_PREEMPTIONS.fetch_update(Ordering::AcqRel,Ordering::Acquire,|v|{
        if apic_preemption_receipt_authorized(epoch,
            MONOTONIC_SESSION_EPOCH.load(Ordering::Acquire),delivered,
            XAPIC_IRQ_TICKS.load(Ordering::Acquire),v,
            XAPIC_TIMER_RUNNING.load(Ordering::Acquire),
            source,MONOTONIC_SOURCE.load(Ordering::Acquire),
            exhausted,MONOTONIC_EXHAUSTED.load(Ordering::Acquire)
        ){v.checked_add(1)}else{None}
    }).ok();
}
#[no_mangle]
pub extern "C" fn x86_64_xapic_timer_preemption_count()->u64{
    XAPIC_PREEMPTIONS.load(Ordering::Relaxed)
}
/// Snapshot whether IRQ delivery has produced actual user preemption.
/// This is diagnostic evidence only; it does not certify fair scheduling.
pub const fn xapic_preemption_evidence(irq:u64,preemptions:u64)->bool{
    irq>0 && preemptions>0 && preemptions<=irq
}
fn apic_live_progress_valid(
 irq:u64,captured:u64,running:bool,source:u8,epoch:u64,exhausted:bool
)->bool {
 apic_preemption_authorized(running,source,epoch,exhausted) &&
 xapic_preemption_evidence(irq,captured)
}
/// Fence diagnostic progress across both hardware and session observations.
fn apic_progress_snapshot_authorized(
    first_epoch:u64,last_epoch:u64,first_source:u8,last_source:u8,
    first_irq:u64,last_irq:u64,first_capture:u64,last_capture:u64,
    first_running:bool,last_running:bool,
    first_exhausted:bool,last_exhausted:bool,
)->bool{
    first_epoch==last_epoch &&
    apic_preemption_authorized(first_running,first_source,first_epoch,first_exhausted) &&
    apic_preemption_authorized(last_running,last_source,last_epoch,last_exhausted) &&
    first_irq!=u64::MAX && last_irq!=u64::MAX &&
    first_capture!=u64::MAX && last_capture!=u64::MAX &&
    last_irq>=first_irq && last_capture>=first_capture &&
    xapic_preemption_evidence(first_irq,first_capture) &&
    xapic_preemption_evidence(last_irq,last_capture)
}
pub fn xapic_preemption_progress()->bool{
    let epoch=MONOTONIC_SESSION_EPOCH.load(Ordering::Acquire);
    let source=MONOTONIC_SOURCE.load(Ordering::Acquire);
    let running=XAPIC_TIMER_RUNNING.load(Ordering::Acquire);
    let exhausted=MONOTONIC_EXHAUSTED.load(Ordering::Acquire);
    let irq=x86_64_xapic_timer_irq_count();
    let captured=x86_64_xapic_timer_preemption_count();
    let last_irq=x86_64_xapic_timer_irq_count();
    let last_captured=x86_64_xapic_timer_preemption_count();
    apic_progress_snapshot_authorized(
        epoch,MONOTONIC_SESSION_EPOCH.load(Ordering::Acquire),
        source,MONOTONIC_SOURCE.load(Ordering::Acquire),
        irq,last_irq,captured,last_captured,
        running,XAPIC_TIMER_RUNNING.load(Ordering::Acquire),
        exhausted,MONOTONIC_EXHAUSTED.load(Ordering::Acquire)
    )
}
/// A running bit alone is insufficient after controller handover or session exhaustion.
fn apic_preemption_ready(running:bool,source:u8,epoch:u64)->bool {
    running && source==2 && epoch!=0 && epoch!=u64::MAX
}
/// An APIC timer interrupt may preempt only with live timer-source authority.
#[no_mangle]
fn apic_preemption_authorized(running:bool,source:u8,epoch:u64,exhausted:bool)->bool {
    !exhausted && apic_preemption_ready(running,source,epoch)
}
pub extern "C" fn x86_64_xapic_timer_preemption_ready()->bool{
    apic_preemption_authorized(
        XAPIC_TIMER_RUNNING.load(Ordering::Acquire),
        MONOTONIC_SOURCE.load(Ordering::Acquire),
        MONOTONIC_SESSION_EPOCH.load(Ordering::Acquire),
        MONOTONIC_EXHAUSTED.load(Ordering::Acquire)
    )
}

// R024: One dedicated, supervisor-only UC alias for the local xAPIC MMIO.
// This research-only map is constructed with IRQs masked before user address
// spaces are cloned. Never dereference a physical or cacheable HHDM alias.
const XAPIC_MMIO_VA:u64=0xffff_fe00_0000_0000;
const XAPIC_PAGE:u64=4096;
const XAPIC_PHYS_MASK:u64=0x000f_ffff_ffff_f000;
const XAPIC_PIT_REFERENCE:u32=11932; // about 10 ms, from the 1.193182 MHz PIT
const XAPIC_PIT_HZ:u64=1_193_182;
static XAPIC_UC_OWNED:AtomicBool=AtomicBool::new(false);
unsafe extern "C" {
    fn boot_hhdm_offset(out:*mut u64)->bool;
    fn pmm_allocate_frame(frame:*mut u64)->bool;
    fn pmm_release_frame(frame:u64)->bool;
}
fn canonical_kernel(value:u64)->bool{
    value>>48==0xffff && value&(1u64<<47)!=0
}
/// Firmware may customize PAT. PCD+PWT means UC only when PAT entry 3 is
/// UC (memory type 0); do not assume firmware kept the reset layout.
pub const fn xapic_pat_index_three_is_uc(pat:u64)->bool{
    (pat>>24)&0xff==0
}
fn xapic_pat_uc()->bool{
    let lo:u32;let hi:u32;
    unsafe{asm!("rdmsr",in("ecx")0x277u32,out("eax")lo,out("edx")hi,
                options(nostack,preserves_flags));}
    xapic_pat_index_three_is_uc(((hi as u64)<<32)|(lo as u64))
}
unsafe fn xapic_frame_address(offset:u64,physical:u64)->Option<*mut u64>{
    if physical==0 || physical&0xfff!=0 ||
       physical&!XAPIC_PHYS_MASK!=0{return None;}
    let va=offset.checked_add(physical)?;
    if !canonical_kernel(va){return None;}
    Some(va as *mut u64)
}
/// # Safety
/// Single BSP, IRQ disabled, PMM and Limine HHDM initialized, four-level
/// paging with PCID disabled. The caller must retain all three allocated
/// page-table frames and the mapping for the entire kernel lifetime.
pub unsafe extern "C" fn x86_64_xapic_install_uc_mapping()->bool{
    if XAPIC_UC_OWNED.load(Ordering::Acquire) || !xapic_pat_uc(){return false;}
    let physical=x86_64_timer_xapic_physical_base();
    if physical==0 || !xapic_register_page_valid(XAPIC_MMIO_VA as usize){return false;}
    let mut offset=0u64;
    if !unsafe{boot_hhdm_offset(&mut offset)} || !canonical_kernel(offset){return false;}
    let cr3:u64;
    unsafe{asm!("mov {}, cr3",out(reg)cr3,options(nostack,nomem,preserves_flags));}
    if cr3&0xfff!=0{return false;} // no PCID or borrowed low CR3 bits
    let Some(root)=(unsafe{xapic_frame_address(offset,cr3&XAPIC_PHYS_MASK)})else{return false;};
    let index=((XAPIC_MMIO_VA>>39)&511) as usize;
    if unsafe{core::ptr::read_volatile(root.add(index))}!=0{return false;}
    let mut pages=[0u64;3];
    let mut count=0usize;
    for i in 0..3{
        if !unsafe{pmm_allocate_frame(&mut pages[i])}{
            for p in pages.iter().take(count){let _=unsafe{pmm_release_frame(*p)};}
            return false;
        }
        let Some(ptr)=(unsafe{xapic_frame_address(offset,pages[i])})else{
            let _=unsafe{pmm_release_frame(pages[i])};
            for p in pages.iter().take(count){let _=unsafe{pmm_release_frame(*p)};}
            return false;
        };
        unsafe{core::ptr::write_bytes(ptr,0,512);}
        count+=1;
    }
    let pdpt=unsafe{xapic_frame_address(offset,pages[0]).unwrap()};
    let pd=unsafe{xapic_frame_address(offset,pages[1]).unwrap()};
    let pt=unsafe{xapic_frame_address(offset,pages[2]).unwrap()};
    // The xAPIC is mapped supervisor-only, writable, and explicitly UC.
    // PWT=PCD=1 chooses PAT index 3, which we verified is UC above.
    unsafe{
        core::ptr::write_volatile(pt,physical|0x1b);
        core::ptr::write_volatile(pd,pages[2]|3);
        core::ptr::write_volatile(pdpt,pages[1]|3);
        core::ptr::write_volatile(root.add(index),pages[0]|3);
        asm!("invlpg [{}]",in(reg)XAPIC_MMIO_VA,options(nostack,preserves_flags));
    }
    if !unsafe{x86_64_xapic_bind_uc_page(XAPIC_MMIO_VA,physical,0x1b,true)}{
        // On failed binding don't reclaim page tables while a published PML4
        // entry references them; fail closed without enabling any IRQ.
        return false;
    }
    XAPIC_UC_OWNED.store(true,Ordering::Release);
    true
}
/// The QEMU test clocks APIC countdown against PIT channel 2 (speaker off),
/// not a guessed APIC clock rate. Channel 2 does NOT deliver PIC IRQ0.
fn xapic_pit_reference_wait()->bool{
    let previous=inb(0x61);
    outb(0x61,(previous|1)&!2);
    outb(PIT_CMD,0xb0); // channel 2, low/high bytes, mode 0
    outb(0x42,(XAPIC_PIT_REFERENCE&0xff) as u8);
    outb(0x42,(XAPIC_PIT_REFERENCE>>8) as u8);
    // Wait for channel output to become low, then terminal-count high.
    let mut observed_low=false;
    for _ in 0..20_000_000 {
        if inb(0x61)&0x20==0 {observed_low=true;break;}
        core::hint::spin_loop();
    }
    let mut completed=false;
    if observed_low {
        for _ in 0..20_000_000{
            if inb(0x61)&0x20!=0 {completed=true;break;}
            core::hint::spin_loop();
        }
    }
    outb(0x61,previous);
    completed
}
/// # Safety
/// BSP ring0, IRQ disabled, UC mapping already installed, calibrated test
/// environment. Installs spurious vector 0xff and APIC vector 0x40 gates
/// must have happened before this call (exceptions_init).
pub unsafe extern "C" fn x86_64_xapic_start_qemu_100hz()->bool{
    if !XAPIC_UC_OWNED.load(Ordering::Acquire) || 
       !unsafe{x86_64_xapic_timer_prepare_masked(0x40)} {return false;}
    let (Some(spurious),Some(tpr))=(
        xapic_reg(XAPIC_SPURIOUS_OFFSET),xapic_reg(0x80)
    )else{return false;};
    unsafe{
        let previous=core::ptr::read_volatile(spurious);
        core::ptr::write_volatile(spurious,(previous&!0xff)|0x1ff);
        core::ptr::write_volatile(tpr,0);
    }
    let initial=u32::MAX;
    if !unsafe{x86_64_xapic_calibration_begin(initial)}{return false;}
    if !xapic_pit_reference_wait(){return false;}
    let current=unsafe{x86_64_xapic_timer_sample_masked()};
    let Some(cfg)=xapic_checked_calibration(
        initial,current,XAPIC_PIT_REFERENCE as u64,XAPIC_PIT_HZ,100
    )else{return false;};
    unsafe{x86_64_xapic_timer_start_calibrated(0x40,16,cfg.initial_count)}
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
    static MMIO_TEST_LOCK:std::sync::Mutex<()>=std::sync::Mutex::new(());
    #[repr(align(4096))]
    struct RegisterPage([u32;1024]);
    struct TimerState([u64;6]);
    impl TimerState {
        fn save()->Self {
            Self([XAPIC_VIRTUAL.load(Ordering::Acquire),
                XAPIC_TIMER_ARMED.load(Ordering::Acquire) as u64,
                XAPIC_TIMER_RUNNING.load(Ordering::Acquire) as u64,
                XAPIC_CALIBRATING.load(Ordering::Acquire) as u64,
                XAPIC_IRQ_TICKS.load(Ordering::Acquire),
                XAPIC_PREEMPTIONS.load(Ordering::Acquire)])
        }
    }
    impl Drop for TimerState {
        fn drop(&mut self){
            XAPIC_VIRTUAL.store(self.0[0],Ordering::Release);
            XAPIC_TIMER_ARMED.store(self.0[1]!=0,Ordering::Release);
            XAPIC_TIMER_RUNNING.store(self.0[2]!=0,Ordering::Release);
            XAPIC_CALIBRATING.store(self.0[3]!=0,Ordering::Release);
            XAPIC_IRQ_TICKS.store(self.0[4],Ordering::Release);
            XAPIC_PREEMPTIONS.store(self.0[5],Ordering::Release);
        }
    }
    #[test]
    fn calibration_lifecycle_preserves_reference_window_and_rejects_stale_samples(){
        let _lock=MMIO_TEST_LOCK.lock().unwrap();
        let mut page=RegisterPage([0;1024]);
        let _restore=TimerState::save();
        // Host-only injection of valid local register storage. Never invoke
        // the binding MSR probe or privileged port I/O in a host test.
        XAPIC_VIRTUAL.store(page.0.as_mut_ptr() as u64,Ordering::Release);
        XAPIC_TIMER_ARMED.store(false,Ordering::Release);
        XAPIC_TIMER_RUNNING.store(false,Ordering::Release);
        XAPIC_CALIBRATING.store(false,Ordering::Release);
        unsafe {
            assert!(x86_64_xapic_timer_prepare_masked(0x40));
            page.0[XAPIC_TIMER_CURRENT_OFFSET/4]=900;
            assert_eq!(x86_64_xapic_timer_sample_masked(),0);
            assert!(!x86_64_xapic_calibration_begin(0));
            assert!(x86_64_xapic_calibration_begin(1000));
            assert_eq!(x86_64_xapic_timer_sample_masked(),900);
            let measuring=page.0;
            assert!(!x86_64_xapic_calibration_begin(2000));
            assert_eq!(page.0,measuring);
            assert!(!x86_64_xapic_timer_start_calibrated(0x41,16,50));
            assert_eq!(page.0,measuring);
            assert_eq!(x86_64_xapic_timer_sample_masked(),900);
            assert!(x86_64_xapic_timer_stop());
            assert_eq!(page.0[XAPIC_TIMER_INITIAL_OFFSET/4],0);
            assert_ne!(page.0[XAPIC_LVT_TIMER_OFFSET/4]&(1<<16),0);
            assert_eq!(x86_64_xapic_timer_sample_masked(),0);
            assert!(x86_64_xapic_calibration_begin(2000));
            assert!(x86_64_xapic_timer_start_calibrated(0x40,16,50));
            assert!(!XAPIC_CALIBRATING.load(Ordering::Acquire));
            assert!(x86_64_xapic_timer_preemption_ready());
            let running=page.0;
            assert!(!x86_64_xapic_calibration_begin(3000));
            assert_eq!(page.0,running);
            assert!(x86_64_xapic_timer_stop());
            assert!(!x86_64_xapic_timer_preemption_ready());
            assert_eq!(x86_64_xapic_timer_sample_masked(),0);
            assert!(x86_64_xapic_calibration_begin(3000));
        }
    }

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
        assert_eq!(xapic_periodic_config(0x41,16,100),None);
        assert_eq!(xapic_periodic_config(0x80,16,100),None);
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
    fn apic_calibration_boundary_and_vector_regressions(){
        // A mathematically valid u32 maximum is accepted, but any
        // count beyond the hardware register width must be rejected.
        assert_eq!(xapic_calibrated_count(u32::MAX as u64,1,1,1),Some(u32::MAX));
        assert_eq!(xapic_calibrated_count(u32::MAX as u64+1,1,1,1),None);
        // Division must not round a sub-tick period up to one tick.
        assert_eq!(xapic_calibrated_count(1,2,1,1),None);
        // The installed APIC gate is 0x40; other vectors are forbidden
        // even when calibration and divider are otherwise valid.
        for vector in [0x20,0x21,0x3f,0x41,0x80,0xfe,0xff]{
            assert_eq!(xapic_config_from_samples(
                vector,16,1000,500,10,100,100
            ),None);
        }
    }
    #[test]
    fn calibration_window_requires_bounded_nonzero_progress(){
        assert!(xapic_calibration_window_valid(1000,999,1));
        assert!(xapic_calibration_window_valid(u32::MAX,1,u64::MAX));
        assert!(!xapic_calibration_window_valid(0,0,1));
        assert!(!xapic_calibration_window_valid(1000,1000,1));
        assert!(!xapic_calibration_window_valid(1000,1001,1));
        assert!(!xapic_calibration_window_valid(1000,0,1));
        assert!(!xapic_calibration_window_valid(1000,500,0));
    }
    #[test]
    fn checked_calibration_rejects_expired_and_stationary_counters(){
        assert!(xapic_checked_calibration(1000,500,10,100,100).is_some());
        assert!(xapic_checked_calibration(1000,0,10,100,100).is_none());
        assert!(xapic_checked_calibration(1000,1000,10,100,100).is_none());
        assert!(xapic_checked_calibration(1000,1001,10,100,100).is_none());
        assert!(xapic_checked_calibration(1000,500,0,100,100).is_none());
        assert!(xapic_checked_calibration(1000,500,10,0,100).is_none());
        assert!(xapic_checked_calibration(1000,500,10,100,0).is_none());
    }
    #[test]
    fn periodic_config_matches_calibration_sample(){
        let cfg=xapic_checked_calibration(1000,500,10,100,100).unwrap();
        assert_eq!(cfg.initial_count,50);
        assert!(xapic_periodic_sample_consistent(cfg,1000,500,10,100,100));
        assert!(!xapic_periodic_sample_consistent(cfg,1000,600,10,100,100));
        assert!(!xapic_periodic_sample_consistent(cfg,1000,0,10,100,100));
        assert!(!xapic_periodic_sample_consistent(cfg,1000,500,10,100,50));
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
        let _lock=MMIO_TEST_LOCK.lock().unwrap();
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
    fn apic_preemption_evidence_requires_real_irq_and_capture(){
        assert!(!xapic_preemption_evidence(0,0));
        assert!(!xapic_preemption_evidence(3,0));
        assert!(!xapic_preemption_evidence(0,1));
        assert!(!xapic_preemption_evidence(1,2));
        assert!(xapic_preemption_evidence(1,1));
        assert!(xapic_preemption_evidence(12,2));
        assert!(xapic_preemption_evidence(u64::MAX,u64::MAX));
    }
    #[test]
    fn stopped_timer_cannot_record_preemption(){
        let _lock=MMIO_TEST_LOCK.lock().unwrap();
        assert!(!XAPIC_TIMER_RUNNING.load(Ordering::Acquire));
        let before=x86_64_xapic_timer_preemption_count();
        xapic_record_preemption();
        assert_eq!(x86_64_xapic_timer_preemption_count(),before);
    }
    #[test]
    fn pat_index_three_requires_uncacheable_type(){
        assert!(xapic_pat_index_three_is_uc(0x0007_0406_0007_0406));
        assert!(!xapic_pat_index_three_is_uc(0x0007_0406_0607_0406));
        assert!(!xapic_pat_index_three_is_uc(0x0007_0406_0407_0406));
    }
    #[test]
    fn canonical_apic_mapping_is_high_half_and_aligned(){
        assert!(canonical_kernel(XAPIC_MMIO_VA));
        assert!(xapic_register_page_valid(XAPIC_MMIO_VA as usize));
        assert!(!canonical_kernel(0x0000_0000_fee0_0000));
        assert!(!canonical_kernel(0x0000_8000_0000_0000));
    }
    #[test]
    fn counter_is_bounded(){
        assert_eq!(TICKS.load(Ordering::Relaxed),0);
        assert!(!CONFIGURED.load(Ordering::Relaxed));
    }
}

#[cfg(test)]
mod r118_cumulative_monotonic_regressions {
    use super::authoritative_monotonic_sample as sample;
    #[test]
    fn rearmed_sources_need_new_irq_and_never_reuse_old_receipts(){
        // Even with a large retained cumulative counter, a rearmed source
        // without a current-session IRQ must refuse an old time receipt.
        assert_eq!(sample(2,false,false,900,true,false),None);
        assert_eq!(sample(2,true,false,901,true,false),Some(901));
        assert_eq!(sample(0,true,false,901,false,false),None);
        assert_eq!(sample(1,false,false,901,false,true),None);
        assert_eq!(sample(1,true,false,902,false,true),Some(902));
        assert_eq!(sample(2,true,false,903,true,true),Some(903));
    }
    #[test]
    fn conflicting_sources_overflow_and_unconfigured_time_fail_closed(){
        assert_eq!(sample(1,true,false,10,true,true),None);
        assert_eq!(sample(1,true,false,10,false,false),None);
        assert_eq!(sample(3,true,false,10,true,true),None);
        assert_eq!(sample(2,true,true,u64::MAX,true,false),None);
        assert_eq!(sample(2,true,false,0,true,false),None);
        assert_eq!(sample(1,true,false,11,false,true),Some(11));
    }
}


// P0-488–494: same-source timer rearm must not authenticate stale ticks.
#[cfg(test)]
mod kernel_p0_batch20_w2_tests{
    use super::*;
    #[test]
    fn kernel_p0_batch20_w2_disallows_same_source_session_aba(){
        assert!(same_timer_session(100,100,2,2));
        assert!(!same_timer_session(100,101,2,2));
        assert!(!same_timer_session(101,102,1,1));
        assert!(!same_timer_session(100,100,1,2));
        assert!(!same_timer_session(u64::MAX,0,2,2));
    }
    #[test]
    fn kernel_p0_batch20_w2_rejects_uncalibrated_or_conflicting_source(){
        assert_eq!(authoritative_monotonic_sample(2,false,false,9,true,false),None);
        assert_eq!(authoritative_monotonic_sample(1,true,false,9,true,true),None);
        assert_eq!(authoritative_monotonic_sample(2,true,false,10,true,false),Some(10));
    }
}


// P0-528: actual delivered-IRQ receipt must belong to the live
// calibrated session, not a previous APIC/PIC instance.
#[cfg(test)]
mod kernel_p0_batch21_w2_tests{
    use super::*;
    #[test]fn kernel_p0_batch21_w2_stale_irq_cannot_authorize_rearm(){
        assert!(current_irq_session(42,42,2,2));
        assert!(current_irq_session(42,42,1,1));
        assert!(!current_irq_session(42,43,2,2));
        assert!(!current_irq_session(42,42,2,1));
        assert!(!current_irq_session(42,42,0,0));
        assert!(!current_irq_session(u64::MAX,0,2,2));
    }
}


// Batch22 W2 P0-564..566: no IRQ from a stale session, false source
// or never-progressing timer may authorize DHCP/scheduler time.
#[cfg(test)]
mod kernel_p0_batch22_w2_tests{
    use super::*;
    #[test]fn kernel_p0_batch22_w2_564_current_irq_receipt_authorizes(){
        assert!(irq_receipt_valid(7,7,true));
        assert_eq!(authoritative_monotonic_sample(2,
            irq_receipt_valid(7,7,true),false,9,true,false),Some(9));
    }
    #[test]fn kernel_p0_batch22_w2_565_same_source_rearm_denies_old_receipt(){
        assert!(!irq_receipt_valid(8,7,true));
        assert!(!irq_receipt_valid(0,0,true));
        assert!(!irq_receipt_valid(8,8,false));
        assert_eq!(authoritative_monotonic_sample(2,
            irq_receipt_valid(8,7,true),false,9,true,false),None);
    }
    #[test]fn kernel_p0_batch22_w2_566_switch_or_exhaustion_denies(){
        assert_eq!(authoritative_monotonic_sample(1,
            irq_receipt_valid(9,9,true),false,12,true,true),None);
        assert_eq!(authoritative_monotonic_sample(2,
            irq_receipt_valid(9,9,true),true,12,true,false),None);
        assert_eq!(authoritative_monotonic_sample(2,
            irq_receipt_valid(9,9,true),false,12,false,true),None);
    }
}


// Batch23 W2 P0-585..588: actual IRQ tick receipt is authorized by
// controller state, not merely by stale monotonic source metadata.
#[cfg(test)]
mod kernel_p0_batch23_w2_tests{
 use super::*;
 #[test]fn kernel_p0_batch23_w2_585_pic_needs_active_irq0(){
   assert!(source_irq_ready(1,true,false));
   assert!(!source_irq_ready(1,false,false));
 }
 #[test]fn kernel_p0_batch23_w2_586_pic_cannot_override_running_apic(){
   assert!(!source_irq_ready(1,true,true));
   assert!(source_irq_ready(2,true,true));
 }
 #[test]fn kernel_p0_batch23_w2_587_apic_requires_running_timer(){
   assert!(!source_irq_ready(2,true,false));
   assert!(!source_irq_ready(2,false,false));
   assert!(source_irq_ready(2,false,true));
 }
 #[test]fn kernel_p0_batch23_w2_588_untrusted_source_ids_denied(){
   for invalid in [0,3,127,255]{
     assert!(!source_irq_ready(invalid,true,true));
   }
   assert!(!source_irq_ready(0,false,false));
 }
}


// Batch24 W2 P0-610..612: the actual IRQ monotonic increment and progress
// publication both require one live authorized session. This is a bounded
// BSP-source check, not an SMP-calibrated high-resolution clock.
#[cfg(test)]
mod kernel_p0_batch24_w2_tests{
    use super::*;
    #[test]fn kernel_p0_batch24_w2_610_pic_epoch_switch_and_apic_priority(){
        assert!(tick_session_authorized(10,10,1,1,true,false,false));
        assert!(!tick_session_authorized(10,11,1,1,true,false,false));
        assert!(!tick_session_authorized(10,10,1,1,true,true,false));
        assert!(tick_session_authorized(11,11,2,2,true,true,false));
    }
    #[test]fn kernel_p0_batch24_w2_611_stopped_and_wrong_source_denied(){
        assert!(!tick_session_authorized(4,4,1,1,false,false,false));
        assert!(!tick_session_authorized(4,4,2,2,true,false,false));
        assert!(!tick_session_authorized(4,4,2,1,true,true,false));
        assert!(!tick_session_authorized(0,0,1,1,true,false,false));
    }
    #[test]fn kernel_p0_batch24_w2_612_saturated_clock_never_rearms(){
        assert!(!tick_session_authorized(4,4,2,2,false,true,true));
        assert!(!tick_session_authorized(u64::MAX,0,2,2,false,true,false));
        assert!(tick_session_authorized(4,4,2,2,false,true,false));
    }
}

// P0-631..635: source/epoch/receipt survive final time publication.
#[cfg(test)]
mod kernel_p0_batch25_w2_tests {
    use super::*;
    #[test]fn kernel_p0_batch25_w2_631_valid_pic_receipt(){
        assert!(monotonic_snapshot_fenced(8,8,1,1,8,8,true,false));
    }
    #[test]fn kernel_p0_batch25_w2_632_missing_initial_receipt(){
        assert!(!monotonic_snapshot_fenced(8,8,1,1,0,8,true,false));
    }
    #[test]fn kernel_p0_batch25_w2_633_invalidated_final_receipt(){
        assert!(!monotonic_snapshot_fenced(8,8,2,2,8,0,true,false));
    }
    #[test]fn kernel_p0_batch25_w2_634_epoch_and_source_aba_denied(){
        assert!(!monotonic_snapshot_fenced(8,9,2,2,8,9,true,false));
        assert!(!monotonic_snapshot_fenced(8,8,1,2,8,8,true,false));
    }
    #[test]fn kernel_p0_batch25_w2_635_stopped_and_exhausted_denied(){
        assert!(!monotonic_snapshot_fenced(8,8,2,2,8,8,false,false));
        assert!(!monotonic_snapshot_fenced(8,8,2,2,8,8,true,true));
    }
}

// P0-660..662: timer shutdown/switchover cannot publish stale time.
#[cfg(test)]
mod kernel_p0_batch26_w2_tests {
    use super::*;
    #[test]fn kernel_p0_batch26_w2_660_live_pic_final_receipt(){
        assert!(monotonic_final_authorized(7,7,1,1,7,7,true,false,true,false));
    }
    #[test]fn kernel_p0_batch26_w2_661_disarmed_pic_denies_publication(){
        assert!(!monotonic_final_authorized(7,7,1,1,7,7,true,false,false,false));
    }
    #[test]fn kernel_p0_batch26_w2_662_apic_takeover_revokes_pic(){
        assert!(!monotonic_final_authorized(7,7,1,1,7,7,true,false,true,true));
        assert!(monotonic_final_authorized(8,8,2,2,8,8,true,false,false,true));
    }
}

#[cfg(test)]
mod kernel_p0_batch27_w2_tests {
    use super::*;
    #[test]fn kernel_p0_batch27_w2_680_first_delivered_irq_authorizes_capture(){
        assert!(preemption_capture_budget(1,0,true));
        assert!(preemption_capture_budget(20,19,true));
    }
    #[test]fn kernel_p0_batch27_w2_681_capture_cannot_exceed_irq_receipts(){
        assert!(!preemption_capture_budget(1,1,true));
        assert!(!preemption_capture_budget(1,2,true));
    }
    #[test]fn kernel_p0_batch27_w2_682_zero_delivery_never_mints_preemption(){
        assert!(!preemption_capture_budget(0,0,true));
        assert!(!xapic_preemption_evidence(0,0));
    }
    #[test]fn kernel_p0_batch27_w2_683_stopped_or_exhausted_cannot_mint(){
        assert!(!preemption_capture_budget(3,0,false));
        assert!(!preemption_capture_budget(u64::MAX,u64::MAX,true));
        assert!(xapic_preemption_evidence(3,3));
        assert!(!xapic_preemption_evidence(3,4));
    }
}

#[cfg(test)]
mod kernel_p0_batch28_w2 {
 use super::*;
 #[test]fn kernel_p0_batch28_w2_705_live_epoch_irq_budget_accepts_capture(){
  assert!(preemption_session_valid(3,3,1,0,true));
  assert!(preemption_session_valid(3,3,5,4,true));
 }
 #[test]fn kernel_p0_batch28_w2_706_rearm_aba_does_not_authorize_capture(){
  assert!(!preemption_session_valid(3,4,10,0,true));
  assert!(!preemption_session_valid(4,3,10,0,true));
 }
 #[test]fn kernel_p0_batch28_w2_707_stopped_zero_and_exhausted_denied(){
  assert!(!preemption_session_valid(0,0,1,0,true));
  assert!(!preemption_session_valid(1,1,1,0,false));
  assert!(!preemption_session_valid(1,1,1,1,true));
  assert!(!preemption_session_valid(1,1,0,0,true));
 }
}

#[cfg(test)]
mod kernel_p0_batch29_w2{
 use super::*;
 #[test] fn kernel_p0_batch29_w2_726_valid_receipt(){assert!(preemption_delivery_receipt_valid(4,4,5,5,4,true));}
 #[test] fn kernel_p0_batch29_w2_727_new_irq_keeps_receipt(){assert!(preemption_delivery_receipt_valid(4,4,5,6,4,true));}
 #[test] fn kernel_p0_batch29_w2_728_counter_rewind_denied(){assert!(!preemption_delivery_receipt_valid(4,4,6,5,4,true));}
 #[test] fn kernel_p0_batch29_w2_729_stale_or_exhausted_denied(){
  assert!(!preemption_delivery_receipt_valid(4,5,5,5,0,true));
  assert!(!preemption_delivery_receipt_valid(4,4,5,5,0,false));
  assert!(!preemption_delivery_receipt_valid(4,4,5,5,5,true));
 }
}

#[cfg(test)]
mod kernel_p0_batch30_w2 {
    use super::*;
    #[test] fn kernel_p0_batch30_w2_750_live_delivery_window_preserved(){
        assert!(preemption_delivery_receipt_valid(9,9,5,6,4,true));
    }
    #[test] fn kernel_p0_batch30_w2_751_saturated_irq_never_mints_capture(){
        assert!(!preemption_delivery_receipt_valid(9,9,u64::MAX,u64::MAX,0,true));
        assert!(!preemption_delivery_receipt_valid(9,9,u64::MAX-1,u64::MAX,0,true));
    }
    #[test] fn kernel_p0_batch30_w2_752_epoch_wrap_and_rewind_rejected(){
        assert!(!preemption_delivery_receipt_valid(u64::MAX,u64::MAX,5,5,4,true));
        assert!(!preemption_delivery_receipt_valid(9,9,6,5,4,true));
        assert!(!preemption_delivery_receipt_valid(9,10,5,5,4,true));
    }
}

#[cfg(test)]
mod kernel_p0_batch31_w2 {
    use super::*;
    #[test]fn kernel_p0_batch31_w2_770_live_apic_capture_preserves_irq_budget(){
        assert!(apic_preemption_receipt_valid(4,4,5,6,4,true,2,2));
    }
    #[test]fn kernel_p0_batch31_w2_771_pic_or_ambiguous_source_denied(){
        for (a,b) in [(0,2),(1,2),(2,1),(2,0),(1,1)]{
            assert!(!apic_preemption_receipt_valid(4,4,5,6,4,true,a,b));
        }
    }
    #[test]fn kernel_p0_batch31_w2_772_stopped_or_saturated_timer_denied(){
        assert!(!apic_preemption_receipt_valid(4,5,5,6,4,true,2,2));
        assert!(!apic_preemption_receipt_valid(4,4,5,6,4,false,2,2));
        assert!(!apic_preemption_receipt_valid(4,4,u64::MAX,u64::MAX,0,true,2,2));
    }
}

#[cfg(test)]
mod kernel_p0_batch32_w2 {
    use super::*;
    #[test] fn kernel_p0_batch32_w2_790_running_xapic_live_epoch_is_ready(){
        assert!(apic_preemption_ready(true,2,1));
        assert!(apic_preemption_ready(true,2,42));
    }
    #[test] fn kernel_p0_batch32_w2_791_pic_handover_revokes_timer_ready(){
        for source in [0,1,3,u8::MAX] {
            assert!(!apic_preemption_ready(true,source,42));
        }
    }
    #[test] fn kernel_p0_batch32_w2_792_stopped_or_exhausted_epoch_denied(){
        assert!(!apic_preemption_ready(false,2,42));
        assert!(!apic_preemption_ready(true,2,0));
        assert!(!apic_preemption_ready(true,2,u64::MAX));
    }
}

#[cfg(test)]
mod kernel_p0_batch33_w2 {
    use super::*;
    #[test] fn kernel_p0_batch33_w2_810_active_xapic_session_admitted(){
        assert!(apic_preemption_authorized(true,2,23,false));
        assert!(apic_preemption_receipt_valid(23,23,3,4,2,true,2,2));
    }
    #[test] fn kernel_p0_batch33_w2_811_sticky_exhaustion_revokes_preemption(){
        assert!(!apic_preemption_authorized(true,2,23,true));
        assert!(!apic_preemption_authorized(true,2,u64::MAX,false));
    }
    #[test] fn kernel_p0_batch33_w2_812_foreign_source_and_exhausted_irq_budget_denied(){
        for source in [0,1,3]{assert!(!apic_preemption_authorized(true,source,23,false));}
        assert!(!apic_preemption_receipt_valid(23,23,3,4,3,true,2,2));
        assert!(!apic_preemption_receipt_valid(23,24,3,4,2,true,2,2));
    }
}

#[cfg(test)]
mod kernel_p0_batch34_w2 {
    use super::*;
    #[test] fn kernel_p0_batch34_w2_830_live_receipt_without_exhaustion_admitted(){
        assert!(apic_preemption_receipt_authorized(
            9,9,5,6,2,true,2,2,false,false));
    }
    #[test] fn kernel_p0_batch34_w2_831_early_or_late_exhaustion_denied(){
        assert!(!apic_preemption_receipt_authorized(
            9,9,5,6,2,true,2,2,true,false));
        assert!(!apic_preemption_receipt_authorized(
            9,9,5,6,2,true,2,2,false,true));
    }
    #[test] fn kernel_p0_batch34_w2_832_cross_epoch_and_apic_source_handover_denied(){
        assert!(!apic_preemption_receipt_authorized(
            9,10,5,6,2,true,2,2,false,false));
        assert!(!apic_preemption_receipt_authorized(
            9,9,5,6,2,true,2,1,false,false));
        assert!(!apic_preemption_receipt_authorized(
            9,9,5,6,2,false,2,2,false,false));
    }
}

#[cfg(test)] mod kernel_p0_batch35_w2 {
 use super::*;
 #[test] fn kernel_p0_batch35_w2_850_live_apic_progress_admitted(){
  assert!(apic_live_progress_valid(9,3,true,2,8,false));
 }
 #[test] fn kernel_p0_batch35_w2_851_source_stop_and_exhaustion_revoked(){
  assert!(!apic_live_progress_valid(9,3,true,1,8,false));
  assert!(!apic_live_progress_valid(9,3,true,2,8,true));
  assert!(!apic_live_progress_valid(9,3,false,2,8,false));
 }
 #[test] fn kernel_p0_batch35_w2_852_bad_epoch_or_irq_evidence_denied(){
  assert!(!apic_live_progress_valid(9,3,true,2,0,false));
  assert!(!apic_live_progress_valid(9,3,true,2,u64::MAX,false));
  assert!(!apic_live_progress_valid(3,4,true,2,8,false));
  assert!(!apic_live_progress_valid(9,0,true,2,8,false));
 }
}

#[cfg(test)]
mod kernel_p0_batch36_w2 {
    use super::*;
    fn receipt()->(u64,u64,u8,u8,u64,u64,u64,u64,bool,bool,bool,bool){
        (7,7,2,2,9,10,3,4,true,true,false,false)
    }
    #[test] fn kernel_p0_batch36_w2_870_live_progress_has_two_sided_receipt(){
        let (a,b,c,d,e,f,g,h,i,j,k,l)=receipt();
        assert!(apic_progress_snapshot_authorized(a,b,c,d,e,f,g,h,i,j,k,l));
    }
    #[test] fn kernel_p0_batch36_w2_871_handover_or_rearm_denies_progress(){
        let (a,b,c,d,e,f,g,h,i,j,k,l)=receipt();
        assert!(!apic_progress_snapshot_authorized(a,b+1,c,d,e,f,g,h,i,j,k,l));
        assert!(!apic_progress_snapshot_authorized(a,b,c,1,e,f,g,h,i,j,k,l));
    }
    #[test] fn kernel_p0_batch36_w2_872_rewinding_or_forged_irq_budget_denied(){
        let (a,b,c,d,e,f,g,h,i,j,k,l)=receipt();
        assert!(!apic_progress_snapshot_authorized(a,b,c,d,11,f,g,h,i,j,k,l));
        assert!(!apic_progress_snapshot_authorized(a,b,c,d,e,f,10,h,i,j,k,l));
        assert!(!apic_progress_snapshot_authorized(a,b,c,d,e,f,g,11,i,j,k,l));
    }
    #[test] fn kernel_p0_batch36_w2_873_stop_exhaustion_and_saturation_denied(){
        let (a,b,c,d,e,f,g,h,i,j,k,l)=receipt();
        assert!(!apic_progress_snapshot_authorized(a,b,c,d,e,f,g,h,i,false,k,l));
        assert!(!apic_progress_snapshot_authorized(a,b,c,d,e,f,g,h,i,j,k,true));
        assert!(!apic_progress_snapshot_authorized(a,b,c,d,e,u64::MAX,g,h,i,j,k,l));
        assert!(!apic_progress_snapshot_authorized(a,b,c,d,e,f,0,h,i,j,k,l));
    }
}
