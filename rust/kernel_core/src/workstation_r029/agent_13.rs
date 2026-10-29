//! R029/13: build-input manifest of real source checksums and toolchain signature.
//! Hashing uses FNV for cache invalidation, not artifact signing.
use super::Error;
use crate::batch7_ramfs::{RamFs,PATH_MAX,CONTENT_MAX};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Input{pub path:[u8;PATH_MAX],pub len:usize,pub rev:u64,pub digest:u64}
pub const EMPTY:Input=Input{path:[0;PATH_MAX],len:0,rev:0,digest:0};
fn hash(bytes:&[u8])->u64{
 let mut h=0xcbf29ce484222325u64;
 for &b in bytes{h=(h^(b as u64)).wrapping_mul(0x100000001b);}h
}
pub struct Cache<const N:usize>{owner:u32,toolchain:u64,items:[Option<Input>;N]}
impl<const N:usize> Cache<N>{
 pub const fn new(owner:u32,toolchain:u64)->Self{
  Self{owner,toolchain,items:[None;N]}
 }
 fn auth(&self,who:u32,toolchain:u64)->Result<(),Error>{
  if who==0||who!=self.owner||toolchain!=self.toolchain{return Err(Error::Denied);}
  Ok(())
 }
 pub fn pin<const F:usize>(&mut self,who:u32,toolchain:u64,
 fs:&RamFs<F>,path:&[u8])->Result<(),Error>{
  self.auth(who,toolchain)?;
  if path.len()<2||path.len()>PATH_MAX{return Err(Error::Invalid);}
  let(rev,size)=fs.metadata(path).map_err(|_|Error::Missing)?;
  let mut bytes=[0u8;CONTENT_MAX];fs.read(path,0,&mut bytes[..size])
   .map_err(|_|Error::Missing)?;
  let mut p=[0u8;PATH_MAX];p[..path.len()].copy_from_slice(path);
  let i=self.items.iter().position(|x|x.is_some_and(|y|
   &y.path[..y.len]==path)).or_else(||self.items.iter().position(Option::is_none))
   .ok_or(Error::Full)?;
  self.items[i]=Some(Input{path:p,len:path.len(),rev,digest:hash(&bytes[..size])});
  Ok(())
 }
 pub fn dirty<const F:usize>(&self,who:u32,toolchain:u64,
 fs:&RamFs<F>)->Result<usize,Error>{
  self.auth(who,toolchain)?;
  let mut total=0;
  for item in self.items.iter().flatten(){
   let path=&item.path[..item.len];
   let Ok((rev,size))=fs.metadata(path)else{total+=1;continue};
   let mut bytes=[0u8;CONTENT_MAX];
   fs.read(path,0,&mut bytes[..size]).map_err(|_|Error::Missing)?;
   if rev!=item.rev||hash(&bytes[..size])!=item.digest{total+=1;}
  }Ok(total)
 }
 pub fn count(&self)->usize{self.items.iter().flatten().count()}
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn source_edit_invalidates_a_pinned_build_artifact(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/p/main.rs",b"old").unwrap();
  let mut cache=Cache::<1>::new(7,21);
  cache.pin(7,21,&fs,b"/p/main.rs").unwrap();
  assert_eq!(cache.dirty(7,21,&fs),Ok(0));
  fs.replace(b"/p/main.rs",b"new").unwrap();
  assert_eq!(cache.dirty(7,21,&fs),Ok(1));
 }
 #[test]fn wrong_toolchain_cannot_reuse_cached_source_evidence(){
  let fs=RamFs::<1>::new();let c=Cache::<1>::new(7,21);
  assert_eq!(c.dirty(7,17,&fs),Err(Error::Denied));
 }
}