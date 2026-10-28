//! R022/19: release admission checks actual source manifest, toolchain and tests.
//! Attestation inputs are trusted by caller; does not cryptographically sign or upload.
use super::Error;
use crate::batch7_ramfs::RamFs;
use crate::workstation_r022::{agent_09,agent_11};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Release{pub owner:u32,pub project:u32,pub source_rev:u64,
 pub toolchain:u64,pub abi:u16,pub test_passes:u32,pub digest:[u8;32],
 pub approved_by:u32,pub state:State}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum State{Draft,Validated,Approved,Revoked}
pub struct Publisher{release:Option<Release>}
impl Publisher{
 pub const fn new()->Self{Self{release:None}}
 pub fn draft(&mut self,owner:u32,project:u32,source_rev:u64,
  toolchain:u64,abi:u16,digest:[u8;32])->Result<(),Error>{
  if self.release.is_some(){return Err(Error::Conflict);}
  if owner==0||project==0||source_rev==0||toolchain==0||
   abi==0||digest==[0;32]{return Err(Error::Invalid);}
  self.release=Some(Release{owner,project,source_rev,toolchain,abi,
   test_passes:0,digest,approved_by:0,state:State::Draft});Ok(())
 }
 pub fn validate<const F:usize>(&mut self,trusted_builder:bool,
  fs:&RamFs<F>,manifest:&agent_09::Export<F>,
  matrix:&agent_11::Matrix<F>,owner:u32,tests:u32)->Result<(),Error>{
  if !trusted_builder||tests==0{return Err(Error::Denied);}
  let mut r=self.release.ok_or(Error::Missing)?;
  if owner!=r.owner||r.source_rev!=fs.version()||
   manifest.captured_revision()!=fs.version()||
   !matrix.all_passed(owner)?{return Err(Error::Stale);}
  manifest.verify(fs)?;
  if r.state!=State::Draft{return Err(Error::Conflict);}
  r.test_passes=tests;r.state=State::Validated;
  self.release=Some(r);Ok(())
 }
 pub fn approve(&mut self,reviewer:u32,owner:u32)->Result<(),Error>{
  let mut r=self.release.ok_or(Error::Missing)?;
  if reviewer==0||reviewer==owner||owner!=r.owner||
   r.state!=State::Validated{return Err(Error::Denied);}
  r.approved_by=reviewer;r.state=State::Approved;
  self.release=Some(r);Ok(())
 }
 pub fn revoke(&mut self,owner:u32)->Result<(),Error>{
  let mut r=self.release.ok_or(Error::Missing)?;
  if owner!=r.owner{return Err(Error::Denied);}
  r.state=State::Revoked;r.approved_by=0;
  self.release=Some(r);Ok(())
 }
 pub fn publishable<const F:usize>(&self,fs:&RamFs<F>)->bool{
  self.release.is_some_and(|r|r.state==State::Approved&&
   r.source_rev==fs.version()&&r.approved_by!=r.owner&&r.test_passes>0)
 }
 pub fn state(&self)->Option<State>{self.release.map(|r|r.state)}
}
#[cfg(test)]mod tests{
 use super::*;
 use crate::workstation_r022::agent_11::{Target,Arch,Edition,State as BuildState};
 fn target()->Target{Target{id:1,parent:0,arch:Arch::X86_64,
  edition:Edition::Developer,toolchain:21,abi:1,state:BuildState::Queued,result:0}}
 #[test]fn real_project_manifest_build_tests_and_review_unlock_release(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/main",b"main").unwrap();
  let mut m=agent_09::Export::<1>::new();m.capture(&fs).unwrap();
  let mut matrix=agent_11::Matrix::<1>::new(7);
  matrix.add(7,target()).unwrap();
  matrix.start(7,1,1,21).unwrap();matrix.finish(7,1,true,0).unwrap();
  let mut release=Publisher::new();release.draft(7,1,1,21,1,[3;32]).unwrap();
  release.validate(true,&fs,&m,&matrix,7,10).unwrap();
  release.approve(8,7).unwrap();assert!(release.publishable(&fs));
 }
 #[test]fn altered_source_after_validation_revokes_effective_publishability(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"a").unwrap();
  let mut p=Publisher::new();p.draft(7,1,1,21,1,[3;32]).unwrap();
  assert!(!p.publishable(&fs));
  fs.replace(b"/a",b"changed").unwrap();assert!(!p.publishable(&fs));
 }
 #[test]fn self_approval_and_unverified_compilation_denied(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"a").unwrap();
  let mut m=agent_09::Export::<1>::new();m.capture(&fs).unwrap();
  let matrix=agent_11::Matrix::<1>::new(7);
  let mut p=Publisher::new();p.draft(7,1,1,21,1,[3;32]).unwrap();
  assert_eq!(p.validate(false,&fs,&m,&matrix,7,1),Err(Error::Denied));
  assert_eq!(p.approve(7,7),Err(Error::Denied));
 }
}
