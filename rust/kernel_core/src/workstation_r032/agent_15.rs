//! R032/15: revision-locked build artifact attestations from trusted build executors.
//! Non-cryptographic digest: evidence tracking, not executable signing.
use super::{Error,agent_02};
use crate::batch7_ramfs::RamFs;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Artifact{pub id:u64,pub file_generation:u64,
 pub digest:u64,pub bytes:usize,pub build_id:u64}
pub struct Registry<const N:usize>{owner:u64,items:[Option<Artifact>;N]}
impl<const N:usize> Registry<N>{
 pub fn new(owner:u64)->Self{Self{owner,items:[None;N]}}
 pub fn register<const F:usize>(&mut self,owner:u64,trusted:bool,
  fs:&RamFs<F>,path:&[u8],id:u64,build_id:u64,
  artifact_bytes:&[u8])->Result<(),Error>{
  if owner==0||owner!=self.owner||!trusted{return Err(Error::Denied);}
  if id==0||build_id==0||artifact_bytes.is_empty()||
   self.items.iter().flatten().any(|x|x.id==id){return Err(Error::Invalid);}
  let (generation,_)=fs.metadata(path).map_err(|_|Error::Missing)?;
  let slot=self.items.iter().position(Option::is_none).ok_or(Error::Full)?;
  self.items[slot]=Some(Artifact{id,file_generation:generation,
   digest:agent_02::digest(artifact_bytes),bytes:artifact_bytes.len(),build_id});
  Ok(())
 }
 pub fn verify<const F:usize>(&self,owner:u64,fs:&RamFs<F>,path:&[u8],
  id:u64,build_id:u64,bytes:&[u8])->Result<bool,Error>{
  if owner!=self.owner||owner==0{return Err(Error::Denied);}
  let artifact=self.items.iter().flatten().find(|a|a.id==id)
   .ok_or(Error::Missing)?;
  let (rev,_)=fs.metadata(path).map_err(|_|Error::Missing)?;
  Ok(artifact.file_generation==rev&&artifact.build_id==build_id&&
   artifact.bytes==bytes.len()&&artifact.digest==agent_02::digest(bytes))
 }
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn trusted_compiler_artifact_is_bound_to_real_source_generation(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/p/main.rs",b"fn main(){}").unwrap();
  let mut reg=Registry::<1>::new(7);
  reg.register(7,true,&fs,b"/p/main.rs",1,4,b"ELF").unwrap();
  assert_eq!(reg.verify(7,&fs,b"/p/main.rs",1,4,b"ELF"),Ok(true));
  fs.replace(b"/p/main.rs",b"new").unwrap();
  assert_eq!(reg.verify(7,&fs,b"/p/main.rs",1,4,b"ELF"),Ok(false));
 }
 #[test]fn forged_evidence_and_wrong_hash_are_rejected(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/p/a",b"a").unwrap();
  let mut reg=Registry::<1>::new(7);
  assert_eq!(reg.register(7,false,&fs,b"/p/a",1,1,b"ELF"),Err(Error::Denied));
  reg.register(7,true,&fs,b"/p/a",1,1,b"ELF").unwrap();
  assert_eq!(reg.verify(7,&fs,b"/p/a",1,1,b"MAL"),Ok(false));
 }
}