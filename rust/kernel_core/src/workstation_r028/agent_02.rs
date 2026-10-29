//! R028/02: bounded volatile per-file snapshots with optimistic restore.
use super::Error;
use crate::batch5_session::Identity;
use crate::batch7_ramfs::{RamFs,PATH_MAX,CONTENT_MAX};
#[derive(Clone,Copy)]
pub struct Version{pub path:[u8;PATH_MAX],pub len:usize,pub bytes:[u8;CONTENT_MAX],
 pub size:usize,pub revision:u64,pub timestamp:u64}
pub struct History<const N:usize>{owner:Identity,snapshots:[Option<Version>;N],next:usize}
impl<const N:usize> History<N>{
 pub const fn new(owner:Identity)->Self{Self{owner,snapshots:[None;N],next:0}}
 fn auth(&self,actor:Identity)->Result<(),Error>{
  if actor.pid==0||actor!=self.owner{return Err(Error::Denied);}Ok(())
 }
 pub fn capture<const F:usize>(&mut self,actor:Identity,fs:&RamFs<F>,
 path:&[u8],timestamp:u64)->Result<usize,Error>{
  self.auth(actor)?;
  if N==0||path.is_empty()||path.len()>PATH_MAX{return Err(Error::Invalid);}
  let (revision,size)=fs.metadata(path).map_err(|_|Error::Missing)?;
  let mut v=Version{path:[0;PATH_MAX],len:path.len(),bytes:[0;CONTENT_MAX],
   size,revision,timestamp};
  v.path[..path.len()].copy_from_slice(path);
  fs.read(path,0,&mut v.bytes[..size]).map_err(|_|Error::Missing)?;
  let slot=self.next;self.snapshots[slot]=Some(v);self.next=(self.next+1)%N;Ok(slot)
 }
 pub fn restore<const F:usize>(&self,actor:Identity,fs:&mut RamFs<F>,
 slot:usize,expected:u64)->Result<u64,Error>{
  self.auth(actor)?;
  if expected!=fs.version(){return Err(Error::Stale);}
  let v=self.snapshots.get(slot).copied().flatten().ok_or(Error::Missing)?;
  fs.metadata(&v.path[..v.len]).map_err(|_|Error::Missing)?;
  fs.replace(&v.path[..v.len],&v.bytes[..v.size]).map_err(|_|Error::Full)
 }
 pub fn preview(&self,actor:Identity,slot:usize,out:&mut[u8])->Result<usize,Error>{
  self.auth(actor)?;
  let v=self.snapshots.get(slot).copied().flatten().ok_or(Error::Missing)?;
  if out.len()<v.size{return Err(Error::Full);}
  out[..v.size].copy_from_slice(&v.bytes[..v.size]);Ok(v.size)
 }
 pub fn count(&self,actor:Identity)->Result<usize,Error>{
  self.auth(actor)?;Ok(self.snapshots.iter().flatten().count())
 }
}
#[cfg(test)]mod tests{
 use super::*;fn id(g:u64)->Identity{Identity{pid:7,generation:g}}
 #[test]fn previous_real_file_content_can_be_restored(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"old").unwrap();
  let mut h=History::<2>::new(id(1));let slot=h.capture(id(1),&fs,b"/a",10).unwrap();
  fs.replace(b"/a",b"new").unwrap();h.restore(id(1),&mut fs,slot,2).unwrap();
  let mut out=[0;3];fs.read(b"/a",0,&mut out).unwrap();assert_eq!(&out,b"old");
 }
 #[test]fn stale_mutation_and_foreign_owner_are_rejected(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"old").unwrap();
  let mut h=History::<1>::new(id(1));let slot=h.capture(id(1),&fs,b"/a",10).unwrap();
  fs.replace(b"/a",b"new").unwrap();
  assert_eq!(h.restore(id(1),&mut fs,slot,1),Err(Error::Stale));
  assert_eq!(h.preview(id(2),slot,&mut[0;10]),Err(Error::Denied));
 }
}