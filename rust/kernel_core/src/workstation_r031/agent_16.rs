//! R031/16: trusted incremental build cache bound to real project file generations.
use super::Error;
use crate::batch7_ramfs::RamFs;
use crate::workstation_r030::agent_14 as source;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum BuildDecision{Rebuild,Cached}
pub struct Cache{owner:u64,verified:Option<(u64,u64,u64)>}
impl Cache{
 pub fn new(owner:u64)->Self{Self{owner,verified:None}}
 pub fn decision<const F:usize>(&self,owner:u64,fs:&RamFs<F>,
  root:&[u8],compiler:&[u8])->Result<BuildDecision,Error>{
  if owner==0||owner!=self.owner{return Err(Error::Denied);}
  let (fingerprint,_)=source::fingerprint(fs,root,compiler)
   .map_err(|_|Error::Invalid)?;
  Ok(if self.verified==Some((fs.version(),fingerprint,compiler.len() as u64)){
   BuildDecision::Cached
  }else{BuildDecision::Rebuild})
 }
 pub fn certify<const F:usize>(&mut self,owner:u64,worker_trusted:bool,
  passed:bool,fs:&RamFs<F>,root:&[u8],compiler:&[u8])->Result<(),Error>{
  if owner==0||owner!=self.owner||!worker_trusted{return Err(Error::Denied);}
  if !passed{self.verified=None;return Err(Error::Conflict);}
  let (digest,_) = source::fingerprint(fs,root,compiler).map_err(|_|Error::Invalid)?;
  self.verified=Some((fs.version(),digest,compiler.len() as u64));Ok(())
 }
 pub fn invalidate(&mut self,owner:u64)->Result<(),Error>{
  if owner!=self.owner||owner==0{return Err(Error::Denied);}
  self.verified=None;Ok(())
 }
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn only_trusted_success_on_unmodified_real_sources_caches_build(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/p/a.rs",b"fn a(){}").unwrap();
  let mut cache=Cache::new(9);
  assert_eq!(cache.decision(9,&fs,b"/p",b"rustc"),Ok(BuildDecision::Rebuild));
  cache.certify(9,true,true,&fs,b"/p",b"rustc").unwrap();
  assert_eq!(cache.decision(9,&fs,b"/p",b"rustc"),Ok(BuildDecision::Cached));
  fs.replace(b"/p/a.rs",b"fn b(){}").unwrap();
  assert_eq!(cache.decision(9,&fs,b"/p",b"rustc"),Ok(BuildDecision::Rebuild));
 }
 #[test]fn untrusted_claim_cannot_mark_build_as_successful(){
  let fs=RamFs::<0>::new();let mut cache=Cache::new(9);
  assert_eq!(cache.certify(9,false,true,&fs,b"/p",b"rustc"),Err(Error::Denied));
  assert_eq!(cache.decision(9,&fs,b"/p",b"rustc"),Ok(BuildDecision::Rebuild));
 }
}