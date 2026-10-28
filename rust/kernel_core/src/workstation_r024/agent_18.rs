//! R024/18: source-revision-bound build artifact cache with verified toolchain metadata.
//! Cache lookup needs trusted builder attestation; stores metadata, not runnable objects.
use super::Error;
use crate::batch7_ramfs::{RamFs,PATH_MAX,CONTENT_MAX};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Artifact{pub owner:u32,pub project:u32,pub file:[u8;PATH_MAX],
 pub len:usize,pub source_rev:u64,pub source_digest:u64,
 pub toolchain:u64,pub artifact_digest:[u8;32],pub valid:bool}
pub struct Cache<const N:usize>{entries:[Option<Artifact>;N]}
pub fn hash(data:&[u8])->u64{
 let mut h=0xcbf29ce484222325u64;
 for &b in data{h=(h^(b as u64)).wrapping_mul(0x100000001b3);}h
}
impl<const N:usize> Cache<N>{
 pub const fn new()->Self{Self{entries:[None;N]}}
 pub fn insert<const F:usize>(&mut self,trusted:bool,owner:u32,
  project:u32,fs:&RamFs<F>,file:&[u8],toolchain:u64,
  artifact_digest:[u8;32])->Result<(),Error>{
  if !trusted||owner==0||project==0||file.len()>PATH_MAX||
   toolchain==0||artifact_digest==[0;32]{return Err(Error::Denied);}
  let (revision,size)=fs.metadata(file).map_err(|_|Error::Missing)?;
  let mut data=[0u8;CONTENT_MAX];
  fs.read(file,0,&mut data[..size]).map_err(|_|Error::Missing)?;
  if self.entries.iter().flatten().any(|a|
   a.owner==owner&&a.project==project&&&a.file[..a.len]==file&&
   a.toolchain==toolchain){return Err(Error::Conflict);}
  let at=self.entries.iter().position(Option::is_none).ok_or(Error::Full)?;
  let mut name=[0;PATH_MAX];name[..file.len()].copy_from_slice(file);
  self.entries[at]=Some(Artifact{owner,project,file:name,len:file.len(),
   source_rev:revision,source_digest:hash(&data[..size]),
   toolchain,artifact_digest,valid:true});Ok(())
 }
 pub fn lookup<const F:usize>(&self,owner:u32,project:u32,fs:&RamFs<F>,
  file:&[u8],toolchain:u64)->Result<Artifact,Error>{
  if owner==0{return Err(Error::Denied);}
  let a=self.entries.iter().flatten().find(|a|a.owner==owner&&
   a.project==project&&a.toolchain==toolchain&&&a.file[..a.len]==file)
   .copied().ok_or(Error::Missing)?;
  if !a.valid{return Err(Error::Stale);}
  let (rev,size)=fs.metadata(file).map_err(|_|Error::Missing)?;
  if rev!=a.source_rev{return Err(Error::Stale);}
  let mut bytes=[0u8;CONTENT_MAX];
  fs.read(file,0,&mut bytes[..size]).map_err(|_|Error::Missing)?;
  if hash(&bytes[..size])!=a.source_digest{return Err(Error::Conflict);}
  Ok(a)
 }
 pub fn invalidate(&mut self,owner:u32,project:u32)->usize{
  let mut n=0;
  for e in &mut self.entries{
   if e.is_some_and(|x|x.owner==owner&&x.project==project){
    *e=None;n+=1;
   }
  }n
 }
 pub fn count(&self,owner:u32)->usize{
  self.entries.iter().flatten().filter(|x|x.owner==owner).count()
 }
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn cache_hit_requires_unchanged_actual_source_and_matching_toolchain(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/main",b"main").unwrap();
  let mut cache=Cache::<2>::new();
  cache.insert(true,7,1,&fs,b"/main",21,[9;32]).unwrap();
  assert!(cache.lookup(7,1,&fs,b"/main",21).is_ok());
  assert_eq!(cache.lookup(7,1,&fs,b"/main",17),Err(Error::Missing));
 }
 #[test]fn user_source_edit_invalidates_cached_compilation(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"source").unwrap();
  let mut c=Cache::<1>::new();c.insert(true,7,1,&fs,b"/a",21,[1;32]).unwrap();
  fs.replace(b"/a",b"changed").unwrap();
  assert_eq!(c.lookup(7,1,&fs,b"/a",21),Err(Error::Stale));
 }
 #[test]fn untrusted_builder_cannot_forge_reusable_artifact(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"a").unwrap();
  let mut c=Cache::<1>::new();
  assert_eq!(c.insert(false,7,1,&fs,b"/a",21,[1;32]),Err(Error::Denied));
  assert_eq!(c.count(7),0);
 }
}
