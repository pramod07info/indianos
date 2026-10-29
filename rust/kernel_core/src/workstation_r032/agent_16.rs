//! R032/16: trusted test reports bound to the exact tested source revision.
use super::{Error,agent_02};
use crate::batch7_ramfs::{RamFs,PATH_MAX,CONTENT_MAX};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Evidence{pub path:[u8;PATH_MAX],pub len:usize,pub revision:u64,
 pub source_hash:u64,pub suite:u32,pub run_id:u64,pub passed:bool}
pub struct Results<const N:usize>{owner:u64,items:[Option<Evidence>;N]}
impl<const N:usize> Results<N>{
 pub fn new(owner:u64)->Self{Self{owner,items:[None;N]}}
 pub fn report<const F:usize>(&mut self,owner:u64,trusted:bool,fs:&RamFs<F>,
 path:&[u8],suite:u32,run_id:u64,passed:bool)->Result<(),Error>{
  if owner==0||owner!=self.owner||!trusted{return Err(Error::Denied);}
  if suite==0||run_id==0||path.len()>PATH_MAX{return Err(Error::Invalid);}
  let (rev,size)=fs.metadata(path).map_err(|_|Error::Missing)?;
  let mut source=[0u8;CONTENT_MAX];fs.read(path,0,&mut source[..size])
   .map_err(|_|Error::Missing)?;
  let checksum=agent_02::digest(&source[..size]);
  let slot=self.items.iter().position(|x|x.is_some_and(|v|v.suite==suite))
   .or_else(||self.items.iter().position(Option::is_none))
   .ok_or(Error::Full)?;
  if let Some(old)=self.items[slot]{
   if run_id<=old.run_id{return Err(Error::Stale);}
  }
  let mut name=[0u8;PATH_MAX];name[..path.len()].copy_from_slice(path);
  self.items[slot]=Some(Evidence{path:name,len:path.len(),revision:rev,
   source_hash:checksum,suite,run_id,passed});Ok(())
 }
 pub fn ready<const F:usize>(&self,owner:u64,fs:&RamFs<F>)->Result<bool,Error>{
  if owner==0||owner!=self.owner{return Err(Error::Denied);}
  let mut any=false;
  for item in self.items.iter().flatten(){
   any=true;
   if !item.passed{return Ok(false);}
   let (rev,size)=fs.metadata(&item.path[..item.len]).map_err(|_|Error::Stale)?;
   if rev!=item.revision{return Ok(false);}
   let mut src=[0u8;CONTENT_MAX];
   fs.read(&item.path[..item.len],0,&mut src[..size]).map_err(|_|Error::Stale)?;
   if agent_02::digest(&src[..size])!=item.source_hash{return Ok(false);}
  }Ok(any)
 }
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn fresh_successful_test_evidence_is_accepted_for_matching_source(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/p/test.rs",b"#[test]fn a(){}").unwrap();
  let mut reports=Results::<1>::new(7);
  reports.report(7,true,&fs,b"/p/test.rs",1,1,true).unwrap();
  assert_eq!(reports.ready(7,&fs),Ok(true));
  fs.replace(b"/p/test.rs",b"#[test]fn b(){}").unwrap();
  assert_eq!(reports.ready(7,&fs),Ok(false));
 }
 #[test]fn untrusted_or_replayed_test_result_fails_closed(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/p/a",b"src").unwrap();
  let mut reports=Results::<1>::new(7);
  assert_eq!(reports.report(7,false,&fs,b"/p/a",1,1,true),Err(Error::Denied));
  reports.report(7,true,&fs,b"/p/a",1,2,true).unwrap();
  assert_eq!(reports.report(7,true,&fs,b"/p/a",1,2,false),Err(Error::Stale));
 }
}