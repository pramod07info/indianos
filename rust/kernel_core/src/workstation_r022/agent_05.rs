//! R022/05: bounded original-content recovery for one real RamFs file.
//! Each snapshot captures actual bytes and expected revision, not durable disk data.
use super::Error;
use crate::batch7_ramfs::{RamFs,PATH_MAX,CONTENT_MAX};
#[derive(Clone,Copy)]
pub struct Snapshot{path:[u8;PATH_MAX],len:usize,
 bytes:[u8;CONTENT_MAX],size:usize,revision:u64}
pub struct History<const N:usize>{snapshots:[Option<Snapshot>;N],count:usize}
impl<const N:usize> History<N>{
 pub const fn new()->Self{Self{snapshots:[None;N],count:0}}
 pub fn capture<const F:usize>(&mut self,fs:&RamFs<F>,
  path:&[u8])->Result<u64,Error>{
  if path.len()>PATH_MAX||self.count>=N{return Err(Error::Full);}
  let (revision,size)=fs.metadata(path).map_err(|_|Error::Missing)?;
  let mut content=[0u8;CONTENT_MAX];
  fs.read(path,0,&mut content[..size]).map_err(|_|Error::Missing)?;
  let mut name=[0u8;PATH_MAX];name[..path.len()].copy_from_slice(path);
  self.snapshots[self.count]=Some(Snapshot{path:name,len:path.len(),
   bytes:content,size,revision});self.count+=1;Ok(revision)
 }
 pub fn restore<const F:usize>(&self,fs:&mut RamFs<F>,
  path:&[u8],snapshot_index:usize,expected_fs:u64)->Result<u64,Error>{
  if fs.version()!=expected_fs{return Err(Error::Stale);}
  let item=self.snapshots.get(snapshot_index).copied().flatten()
   .ok_or(Error::Missing)?;
  if &item.path[..item.len]!=path{return Err(Error::Denied);}
  let (rev,_)=fs.metadata(path).map_err(|_|Error::Missing)?;
  if rev<item.revision{return Err(Error::Stale);}
  fs.replace(path,&item.bytes[..item.size]).map_err(|_|Error::Full)
 }
 pub fn snapshot_bytes(&self,path:&[u8],index:usize,
  out:&mut[u8])->Result<usize,Error>{
  let s=self.snapshots.get(index).copied().flatten().ok_or(Error::Missing)?;
  if &s.path[..s.len]!=path{return Err(Error::Denied);}
  if out.len()<s.size{return Err(Error::Full);}
  out[..s.size].copy_from_slice(&s.bytes[..s.size]);Ok(s.size)
 }
 pub fn count(&self)->usize{self.count}
 pub fn clear(&mut self){self.snapshots=[None;N];self.count=0;}
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn revision_history_restores_actual_original_file_contents(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/src/a",b"before").unwrap();
  let mut history=History::<2>::new();history.capture(&fs,b"/src/a").unwrap();
  fs.replace(b"/src/a",b"after").unwrap();
  history.restore(&mut fs,b"/src/a",0,2).unwrap();
  let mut out=[0;6];fs.read(b"/src/a",0,&mut out).unwrap();
  assert_eq!(&out,b"before");
 }
 #[test]fn restoring_stale_workspace_revision_is_rejected(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"one").unwrap();
  let mut h=History::<1>::new();h.capture(&fs,b"/a").unwrap();
  fs.replace(b"/a",b"two").unwrap();
  assert_eq!(h.restore(&mut fs,b"/a",0,1),Err(Error::Stale));
 }
 #[test]fn snapshot_view_is_fail_atomic_for_short_buffer(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"secret").unwrap();
  let mut h=History::<1>::new();h.capture(&fs,b"/a").unwrap();
  let mut out=[99;2];
  assert_eq!(h.snapshot_bytes(b"/a",0,&mut out),Err(Error::Full));
  assert_eq!(out,[99;2]);
 }
}
