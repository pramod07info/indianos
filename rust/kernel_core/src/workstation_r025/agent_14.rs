//! R025/14: verified source-input inventory and build artifact digest metadata.
//! FNV source digest is noncryptographic; do not treat it as signature verification.
use super::Error;
use crate::batch7_ramfs::{RamFs,PATH_MAX,CONTENT_MAX};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Source{pub path:[u8;PATH_MAX],pub len:usize,pub rev:u64,
 pub size:usize,pub checksum:u64}
pub const EMPTY:Source=Source{path:[0;PATH_MAX],len:0,rev:0,size:0,checksum:0};
fn digest(data:&[u8])->u64{
 let mut hash=0xcbf29ce484222325u64;
 for &b in data{hash=(hash^(b as u64)).wrapping_mul(0x100000001b3);}hash
}
pub struct Manifest<const N:usize>{owner:u32,toolchain:u64,inputs:[Source;N],
 count:usize,baseline:u64}
impl<const N:usize> Manifest<N>{
 pub const fn new(owner:u32,toolchain:u64)->Self{
  Self{owner,toolchain,inputs:[EMPTY;N],count:0,baseline:0}
 }
 pub fn capture<const F:usize>(&mut self,owner:u32,fs:&RamFs<F>,
  prefix:&[u8])->Result<usize,Error>{
  if owner==0||owner!=self.owner||self.toolchain==0{return Err(Error::Denied);}
  if prefix.first()!=Some(&b'/'){return Err(Error::Invalid);}
  let mut stage=[EMPTY;N];let mut count=0;
  for i in 0..fs.count(){
   let mut name=[0u8;PATH_MAX];
   let (len,size,rev)=fs.entry(i,&mut name).map_err(|_|Error::Invalid)?
    .ok_or(Error::Missing)?;
   if !name[..len].starts_with(prefix){continue;}
   if count==N{return Err(Error::Full);}
   let mut bytes=[0u8;CONTENT_MAX];
   fs.read(&name[..len],0,&mut bytes[..size]).map_err(|_|Error::Missing)?;
   stage[count]=Source{path:name,len,rev,size,checksum:digest(&bytes[..size])};
   count+=1;
  }
  if count==0{return Err(Error::Missing);}
  self.inputs=stage;self.count=count;self.baseline=fs.version();Ok(count)
 }
 pub fn verify<const F:usize>(&self,owner:u32,fs:&RamFs<F>)
  ->Result<(),Error>{
  if owner==0||owner!=self.owner{return Err(Error::Denied);}
  if fs.version()!=self.baseline{return Err(Error::Stale);}
  for source in &self.inputs[..self.count]{
   let path=&source.path[..source.len];
   let (rev,size)=fs.metadata(path).map_err(|_|Error::Missing)?;
   if rev!=source.rev||size!=source.size{return Err(Error::Stale);}
   let mut bytes=[0u8;CONTENT_MAX];
   fs.read(path,0,&mut bytes[..size]).map_err(|_|Error::Missing)?;
   if digest(&bytes[..size])!=source.checksum{return Err(Error::Conflict);}
  }Ok(())
 }
 pub fn count(&self)->usize{self.count}
 pub fn baseline(&self)->u64{self.baseline}
 pub fn toolchain(&self)->u64{self.toolchain}
 pub fn total_bytes(&self)->Result<usize,Error>{
  self.inputs[..self.count].iter().try_fold(0usize,|n,s|
   n.checked_add(s.size).ok_or(Error::Overflow))
 }
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn source_manifest_fingerprints_actual_project_bytes(){
  let mut fs=RamFs::<2>::new();
  fs.replace(b"/p/a",b"one").unwrap();fs.replace(b"/p/b",b"two").unwrap();
  let mut m=Manifest::<2>::new(7,21);
  assert_eq!(m.capture(7,&fs,b"/p"),Ok(2));
  assert_eq!(m.total_bytes(),Ok(6));assert_eq!(m.verify(7,&fs),Ok(()));
 }
 #[test]fn source_changes_after_capture_invalidate_artifact_manifest(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/p/a",b"old").unwrap();
  let mut m=Manifest::<1>::new(7,21);m.capture(7,&fs,b"/p").unwrap();
  fs.replace(b"/p/a",b"new").unwrap();
  assert_eq!(m.verify(7,&fs),Err(Error::Stale));
 }
 #[test]fn insufficient_manifest_capacity_does_not_publish_partial_entries(){
  let mut fs=RamFs::<2>::new();
  fs.replace(b"/p/a",b"a").unwrap();fs.replace(b"/p/b",b"b").unwrap();
  let mut m=Manifest::<1>::new(7,21);
  assert_eq!(m.capture(7,&fs,b"/p"),Err(Error::Full));assert_eq!(m.count(),0);
 }
}
