//! R022/09: portable checksum-guarded project export catalog.
//! Captures actual filesystem paths and revisions, not binary executable installers.
use super::Error;
use crate::batch7_ramfs::{RamFs,PATH_MAX};
use crate::workstation_r022::agent_06;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct FileMeta{pub name:[u8;PATH_MAX],pub length:usize,
 pub version:u64,pub size:usize,pub digest:u64}
pub const EMPTY:FileMeta=FileMeta{name:[0;PATH_MAX],length:0,
 version:0,size:0,digest:0};
pub struct Export<const F:usize>{files:[FileMeta;F],len:usize,version:u64}
impl<const F:usize> Export<F>{
 pub const fn new()->Self{Self{files:[EMPTY;F],len:0,version:0}}
 pub fn capture(&mut self,fs:&RamFs<F>)->Result<usize,Error>{
  let mut verified=[agent_06::EMPTY;F];
  let count=agent_06::scan(fs,&mut verified)?;
  let mut staged=[EMPTY;F];
  for i in 0..count{
   let f=verified[i];
   staged[i]=FileMeta{name:f.path,length:f.len,version:f.version,
    size:f.size,digest:f.digest};
  }
  self.files=staged;self.len=count;self.version=fs.version();Ok(count)
 }
 pub fn verify(&self,fs:&RamFs<F>)->Result<(),Error>{
  if fs.version()!=self.version||fs.count()!=self.len{return Err(Error::Stale);}
  for e in &self.files[..self.len]{
   agent_06::verify(fs,agent_06::Entry{path:e.name,len:e.length,
    size:e.size,version:e.version,digest:e.digest})?;
  }Ok(())
 }
 pub fn export(&self,out:&mut[FileMeta])->Result<usize,Error>{
  if out.len()<self.len{return Err(Error::Full);}
  out[..self.len].copy_from_slice(&self.files[..self.len]);Ok(self.len)
 }
 pub fn total_bytes(&self)->Result<usize,Error>{
  self.files[..self.len].iter().try_fold(0usize,|n,e|
   n.checked_add(e.size).ok_or(Error::Overflow))
 }
 pub fn captured_revision(&self)->u64{self.version}
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn export_manifest_tracks_actual_project_files_and_sizes(){
  let mut fs=RamFs::<2>::new();
  fs.replace(b"/main.rs",b"fn main").unwrap();
  fs.replace(b"/lib.rs",b"lib").unwrap();
  let mut export=Export::<2>::new();
  assert_eq!(export.capture(&fs),Ok(2));
  assert_eq!(export.total_bytes(),Ok(10));assert_eq!(export.verify(&fs),Ok(()));
 }
 #[test]fn stale_source_does_not_validate_for_distribution(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"one").unwrap();
  let mut e=Export::<1>::new();e.capture(&fs).unwrap();
  fs.replace(b"/a",b"two").unwrap();
  assert_eq!(e.verify(&fs),Err(Error::Stale));
 }
 #[test]fn short_export_buffer_does_not_publish_partial_manifest(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"x").unwrap();
  let mut e=Export::<1>::new();e.capture(&fs).unwrap();
  let mut buf:[FileMeta;0]=[];assert_eq!(e.export(&mut buf),Err(Error::Full));
 }
}
