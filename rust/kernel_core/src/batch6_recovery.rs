//! R040 tasks 8/21/22/23/25: policy-level crash response and power-cut
//! evidence. Actual watchdog/reset/bootloader and physical hardware remain
//! separate and must provide independently recorded test results.
use crate::batch3_crashlog::{CrashLog,CrashError};
use crate::batch5_session::Identity;
use crate::batch4_performance::{Monitor,Stage};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Error{Invalid,Crash(CrashError),NoEvidence,SlowBoot,RestartExhausted}
pub struct Recovery<const N:usize>{
 pub faults:CrashLog<N>,pub metrics:Monitor,remaining:u8,
}
impl<const N:usize> Recovery<N>{
 pub const fn new(max_restarts:u8)->Self{
  Self{faults:CrashLog::new(),metrics:Monitor::new(),remaining:max_restarts}
 }
 pub fn fault(&mut self,owner:Identity,vector:u16,code:u32,uptime:u64)
  ->Result<bool,Error>{
  // Hardware IRQ vectors are not process exceptions and must never
  // consume a service-restart slot or be logged as a process crash.
  if owner.pid==0||owner.generation==0||vector>=32{
   return Err(Error::Invalid);
  }
  self.faults.record(owner.pid,owner.generation,vector,code,uptime)
   .map_err(Error::Crash)?;
  if self.remaining==0{return Ok(false);}
  self.remaining-=1;Ok(true)
 }
 pub fn boot_sample(&mut self,ticks:u64)->Result<(),Error>{
  self.metrics.record(Stage::Boot,ticks).map_err(|_|Error::Invalid)
 }
 pub fn require_boot_budget(&self,budget:u64)->Result<(),Error>{
  if budget==0{return Err(Error::Invalid);}
  self.metrics.check(Stage::Boot,budget,budget)
   .map_err(|_|Error::SlowBoot)?;Ok(())
 }
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn g8_irq_or_invalid_owner_cannot_consume_crash_retry_budget(){
  let mut r=Recovery::<2>::new(1);
  let owner=Identity{pid:5,generation:9};
  for irq in [32u16,33,48,255,256]{
   assert_eq!(r.fault(owner,irq,0,5),Err(Error::Invalid));
  }
  assert_eq!(r.fault(Identity{pid:0,generation:1},14,0,5),
             Err(Error::Invalid));
  assert_eq!(r.faults.count(),0);
  assert_eq!(r.fault(owner,14,0,10),Ok(true));
  assert_eq!(r.fault(owner,13,0,11),Ok(false));
  assert_eq!(r.faults.count(),2);
 }
 #[test]fn bounded_crash_restarts_then_degrade(){
  let mut r=Recovery::<2>::new(1);let a=Identity{pid:2,generation:1};
  assert_eq!(r.fault(a,14,0,100),Ok(true));
  assert_eq!(r.fault(a,14,0,200),Ok(false));
  assert_eq!(r.faults.count(),2);
  assert_eq!(r.require_boot_budget(100),Err(Error::SlowBoot));
  r.boot_sample(30).unwrap();r.require_boot_budget(100).unwrap();
 }
}