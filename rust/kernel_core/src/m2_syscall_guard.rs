//! M2.4 kernel syscall filter for signed installed native applications.
//! This is applied in the ACTUAL x86-64 INT 0x80 dispatcher, before service
//! dispatch and pointer-validation side effects. Older init/desktop programs
//! are unchanged until explicitly launched with an installed-app lease.
//! Single-BSP only; no SMP, no nested installed process or interrupt reentry.
//!
//! ALLOWED: core ABI/write/exit/yield; file read/write and user GUI input
//! only when their EXACT rights are present. PRIVILEGED package install,
//! raw hardware, quantum jobs, IPC, service control, and unknown syscalls
//! are denied even if an installed app requests the INSTALL or DRIVER bit.
use core::sync::atomic::{AtomicU32,Ordering};
use crate::batch5_session::Identity;
use crate::batch6_security::{ALL,FILE_READ,FILE_WRITE,VIEW};

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Policy{pub identity:Identity,pub rights:u32}
impl Policy{
 pub fn new(identity:Identity,rights:u32)->Option<Self>{
  if identity.pid==0||identity.generation==0||rights&!ALL!=0{
   return None;
  }
  Some(Self{identity,rights})
 }
 /// Return a required right, with 0 for harmless base ABI operations.
 /// Unknown/multiplexed opcodes are DENIED rather than guessed at.
 pub const fn required(syscall:u64,operation:u64)->Option<u32>{
  match syscall{
   0|1|2|3|4=>Some(0),
   6=>Some(VIEW), // supervised terminal/input when enabled in kernel
   11|13=>Some(FILE_WRITE),
   12=>Some(FILE_READ),
   18..=25=>Some(VIEW), // desktop UI operations (own surface checks still apply)
   32=>match operation{
    4|8|10=>Some(FILE_READ), // read, list, read_at
    1|2|3|5|6|7|9|11=>Some(FILE_WRITE),
    _=>None,
   },
   34=>Some(FILE_READ), // read-only Files share, additional PID checks apply
   // SYS_INSTALL 30/31, the generic 33 handle multiplexer, IPC 8/9,
   // quantum 7, driver/control and unknown numbers require more granular
   // proven resource checks, and are DENIED to all installed apps.
   _=>None
  }
 }
 /// Check the installed-app grant on EVERY syscall, not merely when
 /// the application was first launched. A caller cannot retain rights
 /// after capability attenuation, revocation or time-based expiry.
 pub fn allow_live<const N:usize>(&self,
  grants:&crate::batch6_security::Access<N>,caller:Identity,
  now:u64,syscall:u64,operation:u64)->bool{
  if caller!=self.identity{return false;}
  let Some(required)=Self::required(syscall,operation)else{return false;};
  let needed=if required==0{
   crate::batch6_security::VIEW
  }else{required};
  self.rights&needed==needed&&
   grants.require(caller,needed,now).is_ok()
 }
 /// Enforce a kernel-derived live permission plus a bounded user span
 /// for installed-app file reads/writes. Ingress still must validate the
 /// page-table mapping and copy buffers into trusted kernel memory.
 pub fn allow_live_buffer<const N:usize>(&self,
   grants:&crate::batch6_security::Access<N>,caller:Identity,now:u64,
   syscall:u64,operation:u64,addr:u64,length:u64,user_top:u64)->bool{
  if !self.allow_live(grants,caller,now,syscall,operation){
   return false;
  }
  // File read/write services accept pointers; other operations must be
  // verified against their specific syscall ABI before being admitted.
  if !matches!(syscall,11|12|13|32|34){return false;}
  crate::batch4_linux_abi::validate_user_span(addr,length,user_top).is_ok()
 }
 pub fn allow(&self,caller:Identity,syscall:u64,operation:u64)->bool{
  if caller!=self.identity{return false;}
  match Self::required(syscall,operation){
   Some(needed)=>self.rights&needed==needed,
   None=>false,
  }
 }
}
static ACTIVE_PID:AtomicU32=AtomicU32::new(0);
static ACTIVE_RIGHTS:AtomicU32=AtomicU32::new(0);
static BLOCKED:AtomicU32=AtomicU32::new(0);
static LAST_BLOCKED:AtomicU32=AtomicU32::new(0);

/// Lifetime guard around one synchronous, supervisor-owned ring-3 execution.
/// Must not be used with a generic/unsigned ELF or on a second CPU.
pub struct Active{owner:Identity}
pub fn arm(policy:Policy)->Option<Active>{
 if ACTIVE_PID.load(Ordering::Acquire)!=0{return None;}
 LAST_BLOCKED.store(0,Ordering::Relaxed);
 BLOCKED.store(0,Ordering::Relaxed);
 ACTIVE_RIGHTS.store(policy.rights,Ordering::Relaxed);
 // Publish PID *last*, after initializing rights. Single-BSP only.
 ACTIVE_PID.store(policy.identity.pid,Ordering::Release);
 Some(Active{owner:policy.identity})
}
/// Called from x86_64_user_syscall_dispatch using kernel ACTIVE_PROCESS,
/// never a user-supplied PID. Fail closed on missing/mismatched PCB.
pub fn allow_active(pid:u32,syscall:u64,operation:u64)->bool{
 let active=ACTIVE_PID.load(Ordering::Acquire);
 if active==0{return true;} // Explicitly unarmed system processes.
 let needed=Policy::required(syscall,operation);
 let allowed=pid!=0&&pid==active&&needed.is_some_and(|right|
   ACTIVE_RIGHTS.load(Ordering::Relaxed)&right==right);
 if !allowed{
  BLOCKED.fetch_add(1,Ordering::Relaxed);
 }
 allowed
}
pub fn last_denied_calls()->u32{LAST_BLOCKED.load(Ordering::Acquire)}
impl Active{
 pub fn blocked_calls(&self)->u32{BLOCKED.load(Ordering::Relaxed)}
}
impl Drop for Active{
 fn drop(&mut self){
  // The trusted supervisor must drop this guard only after a synchronous
  // ring-3 exit/fault returned with kernel CR3 restored and a reaped PCB.
  if ACTIVE_PID.load(Ordering::Acquire)==self.owner.pid{
   LAST_BLOCKED.store(BLOCKED.load(Ordering::Relaxed),Ordering::Release);
   ACTIVE_PID.store(0,Ordering::Release);
   ACTIVE_RIGHTS.store(0,Ordering::Relaxed);
  }
 }
}
#[cfg(test)]
mod m2_tests{
 use super::*;
 use crate::batch6_security::{INSTALL,DRIVER,SOCKET};
 fn who()->Identity{Identity{pid:5,generation:1}}
 #[test]fn m2_syscall_core_exit_and_console_allowed_privileged_services_denied(){
  let p=Policy::new(who(),0).unwrap();
  for number in [0,1,2,3,4]{assert!(p.allow(who(),number,0));}
  for number in [5,6,7,8,9,10,11,12,13,14,15,16,17,26,30,31,32,33,34,999]{
   assert!(!p.allow(who(),number,1),"syscall {number}");
  }
  let powerful=Policy::new(who(),ALL).unwrap();
  for number in [7,8,9,26,30,31,33,999]{
   assert!(!powerful.allow(who(),number,1),"privileged syscall {number}");
  }
  assert!(powerful.allow(who(),32,4));
  assert!(!powerful.allow(who(),32,12)); // unrecognized file operation
 }
 #[test]fn m2_syscall_file_read_write_and_pid_generation_cannot_escalate(){
  let reader=Policy::new(who(),FILE_READ|VIEW).unwrap();
  for op in [4,8,10]{assert!(reader.allow(who(),32,op));}
  for op in [1,2,3,5,6,7,9,11]{
   assert!(!reader.allow(who(),32,op));
  }
  assert!(reader.allow(who(),12,1));
  assert!(!reader.allow(who(),11,1));
  assert!(reader.allow(who(),34,1));
  let writer=Policy::new(who(),FILE_WRITE|SOCKET).unwrap();
  assert!(writer.allow(who(),11,0));
  assert!(writer.allow(who(),13,0));
  assert!(!writer.allow(who(),12,0));
  assert!(!writer.allow(who(),34,0));
  assert!(!reader.allow(Identity{generation:2,..who()},32,4));
  assert!(!reader.allow(Identity{pid:6,..who()},12,0));
  assert!(Policy::new(who(),DRIVER|INSTALL).is_some());
  assert!(Policy::new(Identity{pid:0,..who()},0).is_none());
  assert!(Policy::new(who(),ALL|64).is_none());
 }

 #[test]fn live_installed_app_syscalls_obey_revoke_and_expiry(){
  use crate::batch6_security::{Access,VIEW,FILE_READ};
  let who=who();
  let policy=Policy::new(who,VIEW|FILE_READ).unwrap();
  let mut grants=Access::<1>::new();
  grants.grant(who,VIEW|FILE_READ,20).unwrap();
  assert!(policy.allow_live(&grants,who,1,12,0));
  assert!(!policy.allow_live(&grants,who,1,11,0));
  assert!(!policy.allow_live(&grants,who,20,12,0));
  assert!(!policy.allow_live(&grants,Identity{generation:2,..who},1,12,0));
  grants.revoke(who).unwrap();
  assert!(!policy.allow_live(&grants,who,1,12,0));
 }
 #[test]fn attenuation_and_privileged_syscalls_fail_closed(){
  use crate::batch6_security::{Access,VIEW,FILE_READ};
  let who=who();let policy=Policy::new(who,VIEW|FILE_READ).unwrap();
  let mut grants=Access::<1>::new();grants.grant(who,VIEW|FILE_READ,100).unwrap();
  grants.attenuate(who,1,VIEW,50).unwrap();
  assert!(!policy.allow_live(&grants,who,2,12,0));
  assert!(policy.allow_live(&grants,who,2,0,0));
  assert!(!policy.allow_live(&grants,who,2,30,0));
  assert!(!policy.allow_live(&grants,who,2,7,0));
  assert!(!policy.allow_live(&grants,who,2,32,200));
 }


 #[test]fn installed_app_buffers_must_be_bounded_before_authorized_file_io(){
  use crate::batch6_security::{Access,VIEW,FILE_READ};
  let actor=who();
  let policy=Policy::new(actor,VIEW|FILE_READ).unwrap();
  let mut grants=Access::<1>::new();
  grants.grant(actor,VIEW|FILE_READ,10).unwrap();
  assert!(policy.allow_live_buffer(&grants,actor,1,12,0,
    0x2000,32,0x10000));
  for (addr,size) in [(0,32),(0x2000,0),(u64::MAX,20),(0xfff0,100)]{
   assert!(!policy.allow_live_buffer(&grants,actor,1,12,0,addr,size,0x10000));
  }
  assert!(!policy.allow_live_buffer(&grants,actor,1,30,0,0x2000,32,0x10000));
  assert!(!policy.allow_live_buffer(&grants,actor,10,12,0,0x2000,32,0x10000));
  assert!(!policy.allow_live_buffer(&grants,
    Identity{generation:2,..actor},1,12,0,0x2000,32,0x10000));
 }

}
