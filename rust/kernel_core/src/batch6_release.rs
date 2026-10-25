//! R040 tasks 10/22/23/25: exact-commit release qualification; does not
//! fabricate CI results or treat a QEMU pass as physical PC support.
use crate::batch3_hardware_matrix::{self,Platform,Evidence as PlatformEvidence};
use crate::batch4_release::{self,Evidence as ReleaseEvidence};
use crate::batch4_performance::{Monitor,Stage};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Error{WrongHardware,FailedReview,NoMetrics,Performance,NoDesktop}
pub struct Proof{pub sha:[u8;20],pub hardware:PlatformEvidence,
 pub release:ReleaseEvidence,pub screenshot_tested:bool,
 pub keyboard_tested:bool,pub recovery_tested:bool}
pub fn qualify(proof:&Proof,platform:Platform,metrics:&Monitor,
 max_boot:u64)->Result<(),Error>{
 batch3_hardware_matrix::qualify(proof.hardware,platform,proof.sha)
  .map_err(|_|Error::WrongHardware)?;
 batch4_release::qualify(proof.release,proof.sha,1)
  .map_err(|_|Error::FailedReview)?;
 if !proof.screenshot_tested||!proof.keyboard_tested||!proof.recovery_tested{
  return Err(Error::NoDesktop);
 }
 if max_boot==0{return Err(Error::NoMetrics);}
 metrics.check(Stage::Boot,max_boot,max_boot)
  .map_err(|_|Error::Performance)?;Ok(())
}
#[cfg(test)]mod tests{
 use super::*;use crate::batch4_release::Status;
 use crate::batch3_hardware_matrix::REQUIRED;
 #[test]fn missing_audit_cannot_get_release_approval(){
  let sha=[4u8;20];let proof=Proof{
   sha,hardware:PlatformEvidence{platform:Platform::QemuX86_64,
    commit:sha,passed:REQUIRED,failed:0,skipped:0},
   release:ReleaseEvidence{sha,kernel:Status::Pass,filesystem:Status::Pass,
    security:Status::Pass,update:Status::Pass,compatibility:Status::Pass,
    platform_mask:1,independently_reviewed:false},
   screenshot_tested:true,keyboard_tested:true,recovery_tested:true
  };
  let mut m=Monitor::new();m.record(Stage::Boot,10).unwrap();
  assert_eq!(qualify(&proof,Platform::QemuX86_64,&m,20),Err(Error::FailedReview));
  assert_eq!(qualify(&proof,Platform::BareMetalX86_64,&m,20),
   Err(Error::WrongHardware));
 }
}