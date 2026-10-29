//! R029/06: bounded real-document undo and redo stack bound to one user process.
//! Each edit publishes real guest RamFs bytes, with optimistic conflict protection.
use super::Error;
use crate::batch5_session::Identity;
use crate::batch7_ramfs::{RamFs,PATH_MAX,CONTENT_MAX};
#[derive(Clone,Copy)]
pub struct Snapshot{path:[u8;PATH_MAX],len:usize,bytes:[u8;CONTENT_MAX],size:usize}
pub struct Undo{owner:Identity,before:Option<Snapshot>,after:Option<Snapshot>,current:u64,undone:bool}
impl Undo{
 pub const fn new(owner:Identity)->Self{
  Self{owner,before:None,after:None,current:0,undone:false}
 }
 fn check(&self,who:Identity)->Result<(),Error>{
  if who.pid==0||who!=self.owner{return Err(Error::Denied);}Ok(())
 }
 fn capture<const F:usize>(fs:&RamFs<F>,path:&[u8])->Result<Snapshot,Error>{
  if path.len()<2||path.len()>PATH_MAX{return Err(Error::Invalid);}
  let (_,size)=fs.metadata(path).map_err(|_|Error::Missing)?;
  let mut s=Snapshot{path:[0;PATH_MAX],len:path.len(),bytes:[0;CONTENT_MAX],size};
  s.path[..path.len()].copy_from_slice(path);
  fs.read(path,0,&mut s.bytes[..size]).map_err(|_|Error::Missing)?;Ok(s)
 }
 pub fn edit<const F:usize>(&mut self,who:Identity,fs:&mut RamFs<F>,
 path:&[u8],revision:u64,data:&[u8])->Result<u64,Error>{
  self.check(who)?;
  if fs.metadata(path).map_err(|_|Error::Missing)?.0!=revision{return Err(Error::Stale);}
  if data.len()>CONTENT_MAX{return Err(Error::Full);}
  let old=Self::capture(fs,path)?;
  let next=fs.replace(path,data).map_err(|_|Error::Full)?;
  self.before=Some(old);self.after=Some(Self::capture(fs,path)?);
  self.current=next;self.undone=false;Ok(next)
 }
 fn swap<const F:usize>(&mut self,who:Identity,fs:&mut RamFs<F>,redo:bool)
  ->Result<u64,Error>{
  self.check(who)?;
  if self.undone!=redo{return Err(Error::NotReady);}
  if fs.version()!=self.current{return Err(Error::Stale);}
  let s=(if redo{self.after}else{self.before}).ok_or(Error::Missing)?;
  let current=fs.metadata(&s.path[..s.len]).map_err(|_|Error::Missing)?;
  if current.0!=self.current{return Err(Error::Stale);}
  let next=fs.replace(&s.path[..s.len],&s.bytes[..s.size]).map_err(|_|Error::Full)?;
  self.current=next;self.undone=!redo;Ok(next)
 }
 pub fn undo<const F:usize>(&mut self,who:Identity,fs:&mut RamFs<F>)->Result<u64,Error>{
  self.swap(who,fs,false)
 }
 pub fn redo<const F:usize>(&mut self,who:Identity,fs:&mut RamFs<F>)->Result<u64,Error>{
  self.swap(who,fs,true)
 }
}
#[cfg(test)]mod tests{
 use super::*;fn id(g:u64)->Identity{Identity{pid:7,generation:g}}
 #[test]fn undo_and_redo_really_restore_source_bytes(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"old").unwrap();
  let mut u=Undo::new(id(1));
  u.edit(id(1),&mut fs,b"/a",1,b"new").unwrap();
  u.undo(id(1),&mut fs).unwrap();
  let mut out=[0;3];fs.read(b"/a",0,&mut out).unwrap();assert_eq!(&out,b"old");
  u.redo(id(1),&mut fs).unwrap();fs.read(b"/a",0,&mut out).unwrap();
  assert_eq!(&out,b"new");
 }
 #[test]fn concurrent_editor_changes_block_stale_undo(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"old").unwrap();
  let mut u=Undo::new(id(1));u.edit(id(1),&mut fs,b"/a",1,b"new").unwrap();
  fs.replace(b"/a",b"other").unwrap();
  assert_eq!(u.undo(id(1),&mut fs),Err(Error::Stale));
 }
}