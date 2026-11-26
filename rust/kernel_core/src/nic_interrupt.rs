//! Exclusive BSP/QEMU legacy IRQ11 notifications; no DMA access in ISR.
//! Used only before user dispatch by the opt-in legacy NIC qualification.
use core::{arch::asm,sync::atomic::{AtomicBool,AtomicU32,AtomicU64,Ordering}};
static PORT:AtomicU32=AtomicU32::new(0);
static EVENTS:AtomicU64=AtomicU64::new(0);
static FAILED:AtomicBool=AtomicBool::new(false);
static WINDOWS:AtomicU32=AtomicU32::new(0);
fn route_allowed(port:u16,irq:u8,flags:u64,active:u32)->bool{
 port!=0&&port.checked_add(0x13).is_some()&&irq==11&&flags&(1<<9)==0&&active==0
}
fn queue_status(value:u8)->Option<bool>{match value{0=>Some(false),1=>Some(true),_=>None}}
fn masked()->bool{let flags:u64;unsafe{asm!("pushfq","pop {}",out(reg)flags,options(preserves_flags));}flags&(1<<9)==0}
unsafe fn out(port:u16,value:u8){unsafe{asm!("out dx, al",in("dx")port,in("al")value,options(nostack,preserves_flags));}}
unsafe fn input(port:u16)->u8{let value:u8;unsafe{asm!("in al, dx",in("dx")port,out("al")value,options(nostack,preserves_flags));}value}
pub(crate) fn active(port:u16)->bool{PORT.load(Ordering::Acquire)==u32::from(port)&&port!=0}
pub(crate) fn notifications()->u64{EVENTS.load(Ordering::Acquire)}
/// # Safety: exclusive IRQ11 QEMU device, live IDT, BSP/CLI, no other
/// hardware users. Does not enable CPU interrupts or negotiate DMA ownership.
pub(crate) unsafe fn enable(port:u16,irq:u8)->bool{
 let flags:u64;unsafe{asm!("pushfq","pop {}",out(reg)flags,options(preserves_flags));}
 if !route_allowed(port,irq,flags,PORT.load(Ordering::Acquire)){return false;}
 if PORT.compare_exchange(0,u32::from(port),Ordering::AcqRel,Ordering::Acquire).is_err(){return false;}
 if !unsafe{crate::timer::prepare_exclusive_nic_pic()}{PORT.store(0,Ordering::Release);return false;}
 EVENTS.store(0,Ordering::Release);FAILED.store(false,Ordering::Release);WINDOWS.store(0,Ordering::Relaxed);
 if queue_status(unsafe{input(port+0x13)}).is_none(){
  PORT.store(0,Ordering::Release);return false;
 }
 true
}
/// One bounded interrupt window. ISR masks IRQ11 before EOI, so a stuck
/// shared line cannot repeatedly reenter and starve the following CLI.
pub(crate) unsafe fn pulse(port:u16,force:bool)->bool{
 if !active(port)||!masked()||FAILED.load(Ordering::Acquire){return false;}
 // Rate limiting is diagnostic scheduling, not an authority generation.
 // Wrapping this counter cannot authorize another device or DMA release.
 if !force&&WINDOWS.fetch_add(1,Ordering::Relaxed)&255!=0{return true;}
 unsafe{out(0xa1,0xf7);asm!("sti","nop","cli",options(nostack));out(0xa1,0xff);}
 !FAILED.load(Ordering::Acquire)
}
/// Mask and revoke notification admission before NIC reset and DMA release.
pub(crate) unsafe fn disable(port:u16)->bool{
 if !active(port)||!masked(){return false;}
 unsafe{out(0xa1,0xff)};
 let status=unsafe{input(port+0x13)};
 PORT.store(0,Ordering::Release);queue_status(status).is_some()
}
#[no_mangle]
pub extern "C" fn x86_64_nic_irq_dispatch(){
 unsafe{out(0xa1,0xff)};
 let port=PORT.load(Ordering::Acquire);
 if port!=0{
  match queue_status(unsafe{input(port as u16+0x13)}){
   Some(true)=>{
    if EVENTS.fetch_update(Ordering::AcqRel,Ordering::Acquire,|n|n.checked_add(1)).is_err(){FAILED.store(true,Ordering::Release);}
   },
   Some(false)=>{},None=>FAILED.store(true,Ordering::Release),
  }
 }
 // IRQ11 is on the slave PIC; no ring/packet/scheduler/allocator is touched.
 unsafe{out(0xa0,0x20);out(0x20,0x20)};
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn exclusive_route_refuses_foreign_irq_reentry_overflow_and_enabled_interrupts(){
  assert!(route_allowed(0xc000,11,2,0));
  for (port,irq,flags,active) in [(0,11,2,0),(0xfff0,11,2,0),(0xc000,10,2,0),
   (0xc000,11,0x202,0),(0xc000,11,2,0xc000)]{assert!(!route_allowed(port,irq,flags,active));}
 }
 #[test]fn removed_device_or_unnegotiated_config_change_is_not_a_queue_interrupt(){
  assert_eq!(queue_status(0),Some(false));assert_eq!(queue_status(1),Some(true));
  for value in [2,3,4,0xff]{assert_eq!(queue_status(value),None);}
 }
}
