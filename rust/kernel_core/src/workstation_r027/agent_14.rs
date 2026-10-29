//! R027/14: version-bound build artifact input manifest for actual RamFs files.
//! FNV checksums are not cryptographic signatures; signed artifacts need separate validation.
use super::Error;
use crate::batch7_ramfs::{RamFs,PATH_MAX,CONTENT_MAX};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Input{pub path:[u8;PATH_MAX],pub len:usize,
 pub revision:u64,pub checksum:u64}
pub const EMPTY:Input=Input{path:[0;PATH_MAX],len:0,revision:0,checksum:0};
pub fn hash(bytes:&[u8])->u64{
 let mut h=0xcbf29ce484222325u64;
 for &b in bytes{h=(h^(b as u64)).wrapping_mul(0x100000001b);}h
}
pub struct Manifest<const N:usize>{pub owner:u32,pub toolchain:u64,
 inputs:[Input;N],count:usize}
impl<const N:usize> Manifest<N>{
 pub const fn new(owner:u32,toolchain:u64)->Self{
  Self{owner,toolchain,inputs:[EMPTY;N],count:0}
 }
 pub fn add<const F:usize>(&mut self,owner:u32,fs:&RamFs<F>,
 path:&[u8])->Result<(),Error>{
  if owner==0||owner!=self.owner||self.toolchain==0{return Err(Error::Denied);}
  if path.len()<2||path.len()>PATH_MAX{return Err(Error::Invalid);}
  if self.inputs[..self.count].iter().any(|i|&i.path[..i.len]==path){
   return Err(Error::Conflict);
  }
  if self.count==N{return Err(Error::Full);}
  let (revision,size)=fs.metadata(path).map_err(|_|Error::Missing)?;
  let mut bytes=[0u8;CONTENT_MAX];
  fs.read(path,0,&mut bytes[..size]).map_err(|_|Error::Missing)?;
  let mut name=[0u8;PATH_MAX];name[..path.len()].copy_from_slice(path);
  self.inputs[self.count]=Input{path:name,len:path.len(),revision,
   checksum:hash(&bytes[..size])};self.count+=1;Ok(())
 }
 pub fn verify<const F:usize>(&self,owner:u32,fs:&RamFs<F>,
 toolchain:u64)->Result<usize,Error>{
  if owner==0||owner!=self.owner||self.toolchain!=toolchain{
   return Err(Error::Denied);
  }
  for input in &self.inputs[..self.count]{
   let name=&input.path[..input.len];
   let (rev,size)=fs.metadata(name).map_err(|_|Error::Missing)?;
   if rev!=input.revision{return Err(Error::Stale);}
   let mut bytes=[0u8;CONTENT_MAX];
   fs.read(name,0,&mut bytes[..size]).map_err(|_|Error::Missing)?;
   if hash(&bytes[..size])!=input.checksum{return Err(Error::Conflict);}
  }Ok(self.count)
 }
 pub fn count(&self)->usize{self.count}
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn build_source_manifest_verifies_real_file_and_compiler(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/main",b"src").unwrap();
  let mut manifest=Manifest::<1>::new(7,21);manifest.add(7,&fs,b"/main").unwrap();
  assert_eq!(manifest.verify(7,&fs,21),Ok(1));
 }
 #[test]fn source_changes_invalidate_old_build_manifest(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/main",b"src").unwrap();
  let mut m=Manifest::<1>::new(7,21);m.add(7,&fs,b"/main").unwrap();
  fs.replace(b"/main",b"changed").unwrap();
  assert_eq!(m.verify(7,&fs,21),Err(Error::Stale));
 }
 #[test]fn mismatch_toolchain_prevents_cache_use(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/main",b"src").unwrap();
  let mut m=Manifest::<1>::new(7,21);m.add(7,&fs,b"/main").unwrap();
  assert_eq!(m.verify(7,&fs,17),Err(Error::Denied));
 }
}
