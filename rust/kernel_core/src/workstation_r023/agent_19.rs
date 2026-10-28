//! R023/19: source-verified release checkpoints with test/build attestation.
//! Requires external trusted runner and reviewer; not a signing implementation.
use super::Error;
use crate::batch7_ramfs::RamFs;
use super::{agent_10,agent_15};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Stage{Draft,Validated,Reviewed,Released,Revoked}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Checkpoint{pub owner:u32,pub project:u32,pub source_version:u64,
 pub digest:[u8;32],pub build_id:u32,pub test_run:u32,
 pub reviewer:u32,pub stage:Stage}
pub struct Releases<const N:usize>{items:[Option<Checkpoint>;N]}
impl<const N:usize> Releases<N>{
 pub const fn new()->Self{Self{items:[None;N]}}
 fn find(&self,owner:u32,project:u32)->Option<usize>{
  self.items.iter().position(|r|r.is_some_and(|x|
   x.owner==owner&&x.project==project))
 }
 pub fn prepare<const F:usize>(&mut self,owner:u32,project:u32,
  fs:&RamFs<F>,digest:[u8;32],build_id:u32,test_run:u32)->Result<(),Error>{
  if owner==0||project==0||fs.version()==0||digest==[0;32]||
   build_id==0||test_run==0{return Err(Error::Invalid);}
  if self.find(owner,project).is_some(){return Err(Error::Conflict);}
  let at=self.items.iter().position(Option::is_none).ok_or(Error::Full)?;
  self.items[at]=Some(Checkpoint{owner,project,source_version:fs.version(),
   digest,build_id,test_run,reviewer:0,stage:Stage::Draft});Ok(())
 }
 pub fn validate<const F:usize,const B:usize,const T:usize>(
  &mut self,trusted:bool,owner:u32,project:u32,fs:&RamFs<F>,
  jobs:&agent_10::Scheduler<B>,tests:&agent_15::Results<T>)->Result<(),Error>{
  if !trusted{return Err(Error::Denied);}
  let at=self.find(owner,project).ok_or(Error::Missing)?;
  let mut c=self.items[at].ok_or(Error::Missing)?;
  if c.stage!=Stage::Draft||fs.version()!=c.source_version{
   return Err(Error::Stale);
  }
  if jobs.get(owner,c.build_id)?.status!=agent_10::Status::Passed||
    !tests.all_passed(owner)?{return Err(Error::NotReady);}
  c.stage=Stage::Validated;self.items[at]=Some(c);Ok(())
 }
 pub fn review(&mut self,owner:u32,project:u32,reviewer:u32)
  ->Result<(),Error>{
  let at=self.find(owner,project).ok_or(Error::Missing)?;
  let mut c=self.items[at].ok_or(Error::Missing)?;
  if reviewer==0||reviewer==owner||c.stage!=Stage::Validated{
   return Err(Error::Denied);
  }
  c.reviewer=reviewer;c.stage=Stage::Reviewed;
  self.items[at]=Some(c);Ok(())
 }
 pub fn publish<const F:usize>(&mut self,trusted:bool,owner:u32,
  project:u32,fs:&RamFs<F>)->Result<(),Error>{
  if !trusted{return Err(Error::Denied);}
  let at=self.find(owner,project).ok_or(Error::Missing)?;
  let mut c=self.items[at].ok_or(Error::Missing)?;
  if c.stage!=Stage::Reviewed||c.source_version!=fs.version()||
   c.reviewer==owner{return Err(Error::Stale);}
  c.stage=Stage::Released;self.items[at]=Some(c);Ok(())
 }
 pub fn revoke(&mut self,owner:u32,project:u32)->Result<(),Error>{
  let at=self.find(owner,project).ok_or(Error::Missing)?;
  let mut c=self.items[at].ok_or(Error::Missing)?;
  c.stage=Stage::Revoked;self.items[at]=Some(c);Ok(())
 }
 pub fn state(&self,owner:u32,project:u32)->Result<Stage,Error>{
  self.find(owner,project).and_then(|i|self.items[i])
   .map(|x|x.stage).ok_or(Error::Missing)
 }
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn signed_source_descriptor_requires_verified_build_tests_and_review(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"source").unwrap();
  let mut jobs=agent_10::Scheduler::<1>::new(7);
  jobs.add(7,1,0,1,100).unwrap();jobs.start(7,1,1).unwrap();
  jobs.finish(true,1,0,2).unwrap();
  let mut tests=agent_15::Results::<1>::new(7,2);
  tests.ingest(true,7,b"test smoke ... ok").unwrap();tests.finish(true,7).unwrap();
  let mut r=Releases::<1>::new();
  r.prepare(7,1,&fs,[1;32],1,2).unwrap();
  r.validate(true,7,1,&fs,&jobs,&tests).unwrap();
  r.review(7,1,8).unwrap();r.publish(true,7,1,&fs).unwrap();
  assert_eq!(r.state(7,1),Ok(Stage::Released));
 }
 #[test]fn malicious_self_review_never_marks_release_approved(){
  let mut r=Releases::<1>::new();
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"a").unwrap();
  r.prepare(7,1,&fs,[1;32],1,2).unwrap();
  assert_eq!(r.review(7,1,7),Err(Error::Denied));
 }
 #[test]fn changed_source_file_rejects_validated_build_manifest(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"a").unwrap();
  let mut r=Releases::<1>::new();r.prepare(7,1,&fs,[1;32],1,2).unwrap();
  fs.replace(b"/a",b"new").unwrap();
  assert_eq!(r.publish(true,7,1,&fs),Err(Error::Stale));
 }
}
