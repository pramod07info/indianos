//! R026/04: per-session source checkpoint and optimistic restore for real files.
//! Checkpoints are volatile RamFs byte copies, not a durable backup service.
use super::Error;
use crate::batch5_session::Identity;
use crate::batch7_ramfs::{RamFs,PATH_MAX,CONTENT_MAX};
#[derive(Clone,Copy)]
pub struct Point{path:[u8;PATH_MAX],len:usize,data:[u8;CONTENT_MAX],
 size:usize,captured_rev:u64,at:u64}
pub struct History<const N:usize>{owner:Identity,points:[Option<Point>;N]}
impl<const N:usize> History<N>{
 pub const fn new(owner:Identity)->Self{Self{owner,points:[None;N]}}
 fn auth(&self,who:Identity)->Result<(),Error>{
  if who.pid==0||who!=self.owner{return Err(Error::Denied);}Ok(())
 }
 pub fn capture<const F:usize>(&mut self,who:Identity,fs:&RamFs<F>,
 path:&[u8],at:u64)->Result<usize,Error>{
  self.auth(who)?;
  if path.len()<2||path.len()>PATH_MAX{return Err(Error::Invalid);}
  let (captured_rev,size)=fs.metadata(path).map_err(|_|Error::Missing)?;
  let slot=self.points.iter().position(Option::is_none).ok_or(Error::Full)?;
  let mut p=Point{path:[0;PATH_MAX],len:path.len(),data:[0;CONTENT_MAX],
   size,captured_rev,at};
  p.path[..path.len()].copy_from_slice(path);
  fs.read(path,0,&mut p.data[..size]).map_err(|_|Error::Missing)?;
  self.points[slot]=Some(p);Ok(slot)
 }
 pub fn restore<const F:usize>(&self,who:Identity,fs:&mut RamFs<F>,
 slot:usize,expected:u64)->Result<u64,Error>{
  self.auth(who)?;
  if fs.version()!=expected{return Err(Error::Stale);}
  let p=self.points.get(slot).copied().flatten().ok_or(Error::Missing)?;
  if fs.metadata(&p.path[..p.len]).is_err(){return Err(Error::Missing);}
  fs.replace(&p.path[..p.len],&p.data[..p.size]).map_err(|_|Error::Full)
 }
 pub fn preview(&self,who:Identity,slot:usize,out:&mut[u8])
  ->Result<usize,Error>{
  self.auth(who)?;
  let p=self.points.get(slot).copied().flatten().ok_or(Error::Missing)?;
  if p.size>out.len(){return Err(Error::Full);}
  out[..p.size].copy_from_slice(&p.data[..p.size]);Ok(p.size)
 }
 pub fn discard(&mut self,who:Identity,slot:usize)->Result<(),Error>{
  self.auth(who)?;
  let p=self.points.get_mut(slot).ok_or(Error::Missing)?;
  if p.is_none(){return Err(Error::Missing);}
  *p=None;Ok(())
 }
 pub fn count(&self,who:Identity)->Result<usize,Error>{
  self.auth(who)?;Ok(self.points.iter().flatten().count())
 }
}
#[cfg(test)]mod tests{
 use super::*;fn id(g:u64)->Identity{Identity{pid:7,generation:g}}
 #[test]fn restores_saved_source_bytes_after_real_editor_change(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"old").unwrap();
  let mut h=History::<1>::new(id(1));let checkpoint=h.capture(id(1),&fs,b"/a",1).unwrap();
  fs.replace(b"/a",b"edited").unwrap();
  h.restore(id(1),&mut fs,checkpoint,2).unwrap();
  let mut out=[0;3];fs.read(b"/a",0,&mut out).unwrap();assert_eq!(&out,b"old");
 }
 #[test]fn stale_restore_view_cannot_overwrite_concurrent_change(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"a").unwrap();
  let mut h=History::<1>::new(id(1));let slot=h.capture(id(1),&fs,b"/a",1).unwrap();
  fs.replace(b"/a",b"new").unwrap();
  assert_eq!(h.restore(id(1),&mut fs,slot,1),Err(Error::Stale));
 }
 #[test]fn process_generation_change_denies_reading_old_checkpoints(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"a").unwrap();
  let mut h=History::<1>::new(id(1));let slot=h.capture(id(1),&fs,b"/a",1).unwrap();
  assert_eq!(h.preview(id(2),slot,&mut [0;10]),Err(Error::Denied));
 }
}
