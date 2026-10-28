//! R019/19 developer release planner with approval, version pinning,
//! rollback metadata and full-release gate. It NEVER deploys live servers.
use super::Error;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Stage{Draft,Reviewed,Approved,Published,Cancelled}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Release{pub owner:u32,pub app:u32,pub version:u64,
 pub digest:[u8;32],pub previous_version:u64,pub reviewer:u32,
 pub stage:Stage,pub checks_passed:bool,pub signed:bool}
impl Release{
 pub fn new(owner:u32,app:u32,version:u64,digest:[u8;32],
  previous_version:u64)->Result<Self,Error>{
  if owner==0||app==0||version==0||version<=previous_version||
   digest==[0;32]{return Err(Error::Invalid);}
  Ok(Self{owner,app,version,digest,previous_version,
   reviewer:0,stage:Stage::Draft,checks_passed:false,signed:false})
 }
 pub fn record_checks(&mut self,owner:u32,passed:bool,signed:bool)
  ->Result<(),Error>{
  if owner==0||owner!=self.owner{return Err(Error::Denied);}
  if self.stage!=Stage::Draft{return Err(Error::Conflict);}
  self.checks_passed=passed;self.signed=signed;Ok(())
 }
 pub fn review(&mut self,reviewer:u32)->Result<(),Error>{
  if reviewer==0||reviewer==self.owner{return Err(Error::Denied);}
  if self.stage!=Stage::Draft||!self.checks_passed||!self.signed{
   return Err(Error::NotReady);
  }
  self.reviewer=reviewer;self.stage=Stage::Reviewed;Ok(())
 }
 pub fn approve(&mut self,reviewer:u32)->Result<(),Error>{
  if self.reviewer!=reviewer||reviewer==0{return Err(Error::Denied);}
  if self.stage!=Stage::Reviewed{return Err(Error::Conflict);}
  self.stage=Stage::Approved;Ok(())
 }
 pub fn publish_manifest(&mut self,supervisor:bool,
  out:&mut[u8])->Result<usize,Error>{
  if !supervisor||self.stage!=Stage::Approved{return Err(Error::Denied);}
  if out.len()<48{return Err(Error::Full);}
  let mut payload=[0u8;48];
  payload[..4].copy_from_slice(&self.app.to_le_bytes());
  payload[4..12].copy_from_slice(&self.version.to_le_bytes());
  payload[12..44].copy_from_slice(&self.digest);
  payload[44..48].copy_from_slice(&(self.previous_version as u32).to_le_bytes());
  out[..48].copy_from_slice(&payload);self.stage=Stage::Published;Ok(48)
 }
 pub fn cancel(&mut self,owner:u32)->Result<(),Error>{
  if owner!=self.owner||owner==0{return Err(Error::Denied);}
  if self.stage==Stage::Published{return Err(Error::Conflict);}
  self.stage=Stage::Cancelled;Ok(())
 }
}
#[cfg(test)]mod tests{
 use super::*;
 fn release()->Release{Release::new(1,7,2,[9;32],1).unwrap()}
 #[test]fn signed_tested_release_requires_independent_reviewer(){
  let mut r=release();r.record_checks(1,true,true).unwrap();
  assert_eq!(r.review(1),Err(Error::Denied));
  r.review(2).unwrap();r.approve(2).unwrap();
  let mut payload=[0u8;48];assert_eq!(r.publish_manifest(true,&mut payload),Ok(48));
  assert_eq!(r.stage,Stage::Published);
 }
 #[test]fn failed_build_or_unsigned_artifact_never_deploys(){
  let mut r=release();r.record_checks(1,false,true).unwrap();
  assert_eq!(r.review(2),Err(Error::NotReady));
  let mut out=[7u8;48];
  assert_eq!(r.publish_manifest(true,&mut out),Err(Error::Denied));
  assert_eq!(out,[7;48]);
 }
 #[test]fn short_publish_buffer_preserves_approved_state(){
  let mut r=release();r.record_checks(1,true,true).unwrap();
  r.review(2).unwrap();r.approve(2).unwrap();
  let mut out=[99u8;10];assert_eq!(r.publish_manifest(true,&mut out),Err(Error::Full));
  assert_eq!(out,[99;10]);assert_eq!(r.stage,Stage::Approved);
 }
}
