//! Eager BSP x87/MMX/SSE ownership for desktop and timer-preempted tasks.
//! AVX/XSAVE and cross-CPU migration are deliberately not enabled here.
#[repr(C,align(16))]
pub(crate) struct Context { bytes:[u8;512] }
impl Context {
 pub(crate) const fn new()->Self {
  let mut bytes=[0;512];
  // Empty x87 stack, masked exceptions, round-to-nearest. FXRSTOR reads
  // MXCSR at byte 24; every reserved byte and all register payloads are zero.
  bytes[0]=0x7f;bytes[1]=0x03;
  bytes[24]=0x80;bytes[25]=0x1f;
  Self{bytes}
 }
}
fn supported(edx:u32,cr4:u64)->bool {
 const REQUIRED:u32=(1<<0)|(1<<24)|(1<<25)|(1<<26);
 edx&REQUIRED==REQUIRED&&cr4&(1<<18)==0
}
#[cfg(not(any(test,feature="std")))]
unsafe extern "C" {
 static mut indianos_user_fxstate:*mut Context;
 static indianos_user_fx_live:u8;
 static indianos_kernel_fxstate:[u8;512];
 fn indianos_fx_enter_user();
 fn indianos_fx_enter_kernel();
}
#[cfg(not(any(test,feature="std")))]
pub(crate) fn mapping_addresses()->[u64;6]{
 let kernel=unsafe{core::ptr::addr_of!(indianos_kernel_fxstate)} as u64;
 [kernel,kernel+511,unsafe{core::ptr::addr_of!(indianos_user_fxstate)} as u64,
  unsafe{core::ptr::addr_of!(indianos_user_fx_live)} as u64,
  indianos_fx_enter_user as *const () as u64,indianos_fx_enter_kernel as *const () as u64]
}
/// Caller holds BSP/IRQ-disabled runtime admission, before any user dispatch.
#[cfg(not(any(test,feature="std")))]
pub(crate) unsafe fn configure()->bool {
 use core::arch::{asm,x86_64::__cpuid_count};
 let flags:u64;let cr4:u64;let cr0:u64;
 unsafe {
  asm!("pushfq","pop {}",out(reg)flags,options(preserves_flags));
  asm!("mov {}, cr4",out(reg)cr4,options(nostack,nomem,preserves_flags));
  asm!("mov {}, cr0",out(reg)cr0,options(nostack,nomem,preserves_flags));
 }
 if flags&(1<<9)!=0||!supported(unsafe{__cpuid_count(1,0).edx},cr4)||
    unsafe{core::ptr::read_volatile(core::ptr::addr_of!(indianos_user_fx_live))}!=0{return false;}
 // AMD FFXSR can omit XMM payloads for privileged FXSAVE. Refuse it rather
 // than claiming complete SSE ownership after a partial hardware save.
 let low:u32;let high:u32;
 unsafe{asm!("rdmsr",in("ecx")0xc000_0080u32,out("eax")low,out("edx")high,options(nostack,preserves_flags));}
 if low&(1<<14)!=0{return false;}let _=high;
 // Do not silently drop enabled extended state: OSXSAVE is refused above.
 unsafe {
  asm!("mov cr0, {}",in(reg)((cr0|2|(1<<5))&!((1<<2)|(1<<3))),options(nostack,preserves_flags));
  asm!("mov cr4, {}",in(reg)(cr4|(1<<9)|(1<<10)),options(nostack,preserves_flags));
 }
 true
}
/// The supervisor-owned aligned context must remain mapped in every process
/// root until unbind. The assembly boundary alone reads/writes its payload.
#[cfg(not(any(test,feature="std")))]
pub(crate) unsafe fn bind(context:*mut Context)->bool {
 if context.is_null()||context as usize%16!=0||
    unsafe{core::ptr::read_volatile(core::ptr::addr_of!(indianos_user_fx_live))}!=0{return false;}
 unsafe{core::ptr::write_volatile(core::ptr::addr_of_mut!(indianos_user_fxstate),context)};
 true
}
#[cfg(not(any(test,feature="std")))]
pub(crate) unsafe fn unbind(){
 unsafe{core::ptr::write_volatile(core::ptr::addr_of_mut!(indianos_user_fxstate),core::ptr::null_mut())};
}
#[cfg(test)]
mod tests {
 use super::*;
 #[test]fn clean_context_has_architectural_defaults_and_no_previous_payload(){
  let context=Context::new();
  assert_eq!(core::mem::size_of::<Context>(),512);
  assert_eq!(core::mem::align_of::<Context>(),16);
  assert_eq!(&context.bytes[..2],&[0x7f,3]);
  assert_eq!(&context.bytes[24..28],&[0x80,0x1f,0,0]);
  assert!(context.bytes.iter().enumerate().all(|(i,&v)|matches!(i,0|1|24|25)||v==0));
 }
 #[test]fn missing_features_or_enabled_extended_state_are_refused(){
  let required=(1<<0)|(1<<24)|(1<<25)|(1<<26);
  assert!(supported(required,0));
  for bit in [0,24,25,26]{assert!(!supported(required&!(1<<bit),0));}
  assert!(!supported(required,1<<18));
  assert!(supported(required,(1<<9)|(1<<10)));
 }
 #[test]fn new_process_state_never_inherits_predecessor_bytes(){
  let mut old=Context::new();old.bytes[160..].fill(0xa5);
  let new=Context::new();assert!(new.bytes[160..].iter().all(|&v|v==0));
  assert_eq!(old.bytes[160],0xa5);
 }
}
