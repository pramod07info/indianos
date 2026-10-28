//! R025/19: release gating against real source manifest + build/test evidence.
//! Review policy only; no cryptographic signer or remote release upload here.
use super::{Error,agent_13,agent_14};
use crate::batch7_ramfs::RamFs;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum State{Draft,Validated,Reviewed,Published,Revoked}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Release{pub owner:u32,pub project:u32,pub source_rev:u64,
 pub reviewer:u32,pub tests:u32,pub stage:State}
pub struct Gate{current:Option<Release>}
impl Gate{
 pub const fn new()->Self{Self{current:None}}
 pub fn draft<const F:usize>(&mut self,owner:u32,project:u32,
  fs:&RamFs<F>)->Result<(),Error>{
  if self.current.is_some(){return Err(Error::Conflict);}
  if owner==0||project==0||fs.version()==0{return Err(Error::Invalid);}
  self.current=Some(Release{owner,project,source_rev:fs.version(),
   reviewer:0,tests:0,stage:State::Draft});Ok(())
 }
 pub fn validate<const F:usize,const S:usize,const J:usize>(
  &mut self,trusted_worker:bool,owner:u32,fs:&RamFs<F>,
  manifest:&agent_14::Manifest<S>,jobs:&agent_13::Graph<J>,
  passed_tests:u32)->Result<(),Error>{
  if !trusted_worker||passed_tests==0{return Err(Error::Denied);}
  let mut release=self.current.ok_or(Error::Missing)?;
  if release.owner!=owner||release.stage!=State::Draft||
   release.source_rev!=fs.version(){return Err(Error::Stale);}
  manifest.verify(owner,fs)?;
  if !jobs.all_passed(owner)?{return Err(Error::NotReady);}
  release.tests=passed_tests;release.stage=State::Validated;
  self.current=Some(release);Ok(())
 }
 pub fn review(&mut self,owner:u32,reviewer:u32)->Result<(),Error>{
  let mut r=self.current.ok_or(Error::Missing)?;
  if owner==0||owner!=r.owner||reviewer==0||reviewer==owner||
   r.stage!=State::Validated{return Err(Error::Denied);}
  r.reviewer=reviewer;r.stage=State::Reviewed;self.current=Some(r);Ok(())
 }
 pub fn publish<const F:usize>(&mut self,trusted:bool,owner:u32,
  fs:&RamFs<F>)->Result<(),Error>{
  if !trusted{return Err(Error::Denied);}
  let mut r=self.current.ok_or(Error::Missing)?;
  if owner==0||r.owner!=owner||r.stage!=State::Reviewed||
   r.source_rev!=fs.version()||r.reviewer==owner{return Err(Error::Stale);}
  r.stage=State::Published;self.current=Some(r);Ok(())
 }
 pub fn revoke(&mut self,owner:u32)->Result<(),Error>{
  let mut r=self.current.ok_or(Error::Missing)?;
  if owner==0||r.owner!=owner{return Err(Error::Denied);}
  r.stage=State::Revoked;self.current=Some(r);Ok(())
 }
 pub fn state(&self)->Option<State>{self.current.map(|r|r.stage)}
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn release_requires_current_files_verified_build_and_independent_reviewer(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/p/a",b"code").unwrap();
  let mut m=agent_14::Manifest::<1>::new(7,21);m.capture(7,&fs,b"/p").unwrap();
  let mut jobs=agent_13::Graph::<1>::new(7);
  jobs.add(7,1,0,agent_13::Stage::Compile,21).unwrap();
  jobs.start(7,1,21).unwrap();jobs.finish(true,1,0).unwrap();
  let mut gate=Gate::new();gate.draft(7,1,&fs).unwrap();
  gate.validate(true,7,&fs,&m,&jobs,1).unwrap();
  gate.review(7,8).unwrap();gate.publish(true,7,&fs).unwrap();
  assert_eq!(gate.state(),Some(State::Published));
 }
 #[test]fn self_approval_cannot_mark_release_ready(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"a").unwrap();
  let mut gate=Gate::new();gate.draft(7,1,&fs).unwrap();
  assert_eq!(gate.review(7,7),Err(Error::Denied));
 }
 #[test]fn changed_file_blocks_publish_after_lost_build_freshness(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"a").unwrap();
  let mut gate=Gate::new();gate.draft(7,1,&fs).unwrap();
  fs.replace(b"/a",b"new").unwrap();
  assert_eq!(gate.publish(true,7,&fs),Err(Error::Stale));
 }
}
