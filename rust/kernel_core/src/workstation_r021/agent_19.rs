//! R021/19 signed-descriptor developer workstation update rollout policy.
//! It never downloads packages, writes firmware, or activates boot slots.
use super::Error;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Track{Stable,Preview,Internal}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Stage{Offered,Approved,Canary,Full,Paused,Withdrawn}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Update{pub id:u64,pub version:u64,pub current:u64,
 pub digest:[u8;32],pub track:Track,pub rollout_percent:u8,
 pub state:Stage,pub admin:u32,pub signer_verified:bool}
pub struct Channel{release:Option<Update>}
impl Channel{
 pub const fn new()->Self{Self{release:None}}
 pub fn offer(&mut self,admin:u32,version:u64,current:u64,
  digest:[u8;32],track:Track,verified:bool)->Result<(),Error>{
  if admin==0||version<=current||digest==[0;32]||!verified{
   return Err(Error::Denied);
  }
  if self.release.is_some(){return Err(Error::Conflict);}
  self.release=Some(Update{id:version,version,current,digest,track,
   rollout_percent:0,state:Stage::Offered,admin,signer_verified:true});Ok(())
 }
 pub fn approve(&mut self,admin:u32)->Result<(),Error>{
  let r=self.release.as_mut().ok_or(Error::Missing)?;
  if admin!=r.admin||r.state!=Stage::Offered{return Err(Error::Denied);}
  r.state=Stage::Approved;Ok(())
 }
 pub fn rollout(&mut self,admin:u32,percent:u8)->Result<(),Error>{
  let r=self.release.as_mut().ok_or(Error::Missing)?;
  if admin!=r.admin||percent==0||percent>100||
   percent<r.rollout_percent||!matches!(r.state,Stage::Approved|Stage::Canary){
   return Err(Error::Denied);
  }
  r.rollout_percent=percent;
  r.state=if percent==100{Stage::Full}else{Stage::Canary};
  Ok(())
 }
 pub fn eligible(&self,cohort:u32,target:Track)->bool{
  self.release.is_some_and(|r|
   (r.state==Stage::Canary||r.state==Stage::Full)&&
   (cohort%100)<(r.rollout_percent as u32)&&r.track==target)
 }
 pub fn pause(&mut self,admin:u32)->Result<(),Error>{
  let r=self.release.as_mut().ok_or(Error::Missing)?;
  if admin!=r.admin||!matches!(r.state,Stage::Canary|Stage::Full){
   return Err(Error::Denied);
  }
  r.state=Stage::Paused;Ok(())
 }
 pub fn withdraw(&mut self,admin:u32)->Result<(),Error>{
  let r=self.release.as_mut().ok_or(Error::Missing)?;
  if admin!=r.admin{return Err(Error::Denied);}
  r.state=Stage::Withdrawn;r.rollout_percent=0;Ok(())
 }
 pub fn snapshot(&self)->Option<Update>{self.release}
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn signed_update_reaches_canary_then_full_rollout(){
  let mut c=Channel::new();
  c.offer(7,2,1,[2;32],Track::Stable,true).unwrap();
  c.approve(7).unwrap();c.rollout(7,10).unwrap();
  assert!(c.eligible(5,Track::Stable));assert!(!c.eligible(50,Track::Stable));
  c.rollout(7,100).unwrap();assert!(c.eligible(50,Track::Stable));
 }
 #[test]fn unverified_update_and_non_admin_rollout_are_denied(){
  let mut c=Channel::new();
  assert_eq!(c.offer(7,2,1,[2;32],Track::Stable,false),Err(Error::Denied));
  c.offer(7,2,1,[2;32],Track::Stable,true).unwrap();
  assert_eq!(c.approve(8),Err(Error::Denied));assert!(!c.eligible(2,Track::Stable));
 }
 #[test]fn revoked_canary_is_no_longer_eligible_for_download(){
  let mut c=Channel::new();
  c.offer(7,2,1,[2;32],Track::Stable,true).unwrap();
  c.approve(7).unwrap();c.rollout(7,50).unwrap();
  c.withdraw(7).unwrap();assert!(!c.eligible(1,Track::Stable));
 }
}
