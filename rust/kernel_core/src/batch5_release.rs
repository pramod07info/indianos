//! Batch 5 tasks 10,21–25: compose real evidence at exact SHA.
//! Hardware/screenshot/boot reports MUST come from external verified tests.
use crate::batch4_release::{self,Evidence};
use crate::batch3_hardware_matrix::{self,Evidence as HardwareEvidence,Platform};
use crate::batch4_performance::{Monitor,Stage};
use crate::batch3_crashlog::{CrashLog,Crash,CrashError};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Error{Hardware,Release,Performance,NoCrashLog}
/// Requires independent security reviews; never marks evidence true itself.
pub fn qualify_release(sha:[u8;20],hardware:HardwareEvidence,
  requested_platform:Platform,policy:Evidence,
  perf:&Monitor,cold_boot_budget:u64)->Result<(),Error>{
 batch3_hardware_matrix::qualify(hardware,requested_platform,sha)
  .map_err(|_|Error::Hardware)?;
 batch4_release::qualify(policy,sha,1).map_err(|_|Error::Release)?;
 perf.check(Stage::Boot,cold_boot_budget,cold_boot_budget)
  .map_err(|_|Error::Performance)?;
 Ok(())
}
pub fn trusted_fault<const N:usize>(log:&mut CrashLog<N>,pid:u32,epoch:u64,
 vector:u16,code:u32,uptime:u64)->Result<Crash,CrashError>{
 log.record(pid,epoch,vector,code,uptime)
}
#[cfg(test)]mod tests{
 use super::*;
 use crate::batch4_release::Status;
 use crate::batch3_hardware_matrix::REQUIRED;
 #[test]fn release_refuses_any_missing_independent_review(){
  let sha=[1;20];
  let hardware=HardwareEvidence{platform:Platform::QemuX86_64,
   commit:sha,passed:REQUIRED,failed:0,skipped:0};
  let p=Evidence{sha,kernel:Status::Pass,filesystem:Status::Pass,
   security:Status::Pass,update:Status::Pass,compatibility:Status::Pass,
   platform_mask:1,independently_reviewed:false};
  let mut perf=Monitor::new();perf.record(Stage::Boot,10).unwrap();
  assert_eq!(qualify_release(sha,hardware,Platform::QemuX86_64,p,&perf,20),
   Err(Error::Release));
  assert_eq!(qualify_release([2;20],hardware,Platform::QemuX86_64,p,&perf,20),
   Err(Error::Hardware));
 }
 #[test]fn fault_records_never_carry_unbounded_user_data(){
  let mut ring=CrashLog::<2>::new();
  assert_eq!(trusted_fault(&mut ring,4,1,14,2,100).unwrap().vector,14);
 }
}