//! R022/03: actual editor buffer save/refresh against versioned guest files.
//! Guards unsaved edits and avoids stale overwrites after external filesystem edits.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX};
use crate::workstation_r018::editor::Buffer;
pub struct Session<const B:usize>{
 owner:u32,editor:Buffer<B>,saved_editor_rev:u64,fs_rev:u64,active:bool
}
impl<const B:usize> Session<B>{
 pub const fn new(owner:u32)->Self{Self{owner,editor:Buffer::new(owner),
  saved_editor_rev:0,fs_rev:0,active:false}}
 pub fn open<const F:usize>(&mut self,owner:u32,fs:&RamFs<F>,
  path:&[u8])->Result<(),Error>{
  if owner==0||owner!=self.owner{return Err(Error::Denied);}
  if self.active&&self.editor.revision()!=self.saved_editor_rev{
   return Err(Error::Conflict);
  }
  let (revision,len)=fs.metadata(path).map_err(|_|Error::Missing)?;
  if len>B{return Err(Error::Full);}
  let mut text=[0u8;B];fs.read(path,0,&mut text[..len])
   .map_err(|_|Error::Missing)?;
  let newrev=self.editor.load(owner,self.editor.revision(),&text[..len])
   .map_err(|_|Error::Stale)?;
  self.saved_editor_rev=newrev;self.fs_rev=revision;self.active=true;Ok(())
 }
 pub fn append(&mut self,owner:u32,bytes:&[u8])->Result<u64,Error>{
  if !self.active||owner!=self.owner||owner==0{return Err(Error::Denied);}
  self.editor.append(owner,self.editor.revision(),bytes)
   .map_err(|_|Error::Full)
 }
 pub fn save<const F:usize>(&mut self,owner:u32,fs:&mut RamFs<F>,
  path:&[u8])->Result<u64,Error>{
  if !self.active||owner!=self.owner||owner==0{return Err(Error::Denied);}
  let (actual,_)=fs.metadata(path).map_err(|_|Error::Missing)?;
  if actual!=self.fs_rev{return Err(Error::Stale);}
  let mut bytes=[0u8;B];
  let n=self.editor.copy(owner,0,self.editor.len(),&mut bytes)
   .map_err(|_|Error::Denied)?;
  if n>CONTENT_MAX{return Err(Error::Full);}
  let next=fs.replace(path,&bytes[..n]).map_err(|_|Error::Full)?;
  self.saved_editor_rev=self.editor.revision();self.fs_rev=next;Ok(next)
 }
 pub fn dirty(&self,owner:u32)->Result<bool,Error>{
  if !self.active||owner!=self.owner||owner==0{return Err(Error::Denied);}
  Ok(self.editor.revision()!=self.saved_editor_rev)
 }
 pub fn view(&self,owner:u32,out:&mut[u8])->Result<usize,Error>{
  if !self.active||owner!=self.owner||owner==0{return Err(Error::Denied);}
  self.editor.copy(owner,0,self.editor.len(),out).map_err(|_|Error::Full)
 }
 pub fn close(&mut self,owner:u32,force:bool)->Result<(),Error>{
  if owner!=self.owner||owner==0{return Err(Error::Denied);}
  if self.active&&self.editor.revision()!=self.saved_editor_rev&&!force{
   return Err(Error::Conflict);
  }
  self.active=false;Ok(())
 }
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn editor_save_roundtrip_updates_real_source_file(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/main",b"fn").unwrap();
  let mut s=Session::<64>::new(7);s.open(7,&fs,b"/main").unwrap();
  s.append(7,b" main").unwrap();assert_eq!(s.dirty(7),Ok(true));
  s.save(7,&mut fs,b"/main").unwrap();assert_eq!(s.dirty(7),Ok(false));
  let mut out=[0;7];assert_eq!(fs.read(b"/main",0,&mut out),Ok(7));
  assert_eq!(&out,b"fn main");
 }
 #[test]fn external_change_blocks_stale_editor_overwrite(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/main",b"source").unwrap();
  let mut s=Session::<64>::new(7);s.open(7,&fs,b"/main").unwrap();
  s.append(7,b"x").unwrap();fs.replace(b"/main",b"new").unwrap();
  assert_eq!(s.save(7,&mut fs,b"/main"),Err(Error::Stale));
  assert_eq!(s.close(7,false),Err(Error::Conflict));
 }
 #[test]fn foreign_owner_cannot_read_unsaved_buffer_or_save(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/main",b"source").unwrap();
  let mut s=Session::<64>::new(7);s.open(7,&fs,b"/main").unwrap();
  assert_eq!(s.view(8,&mut [0;64]),Err(Error::Denied));
  assert_eq!(s.save(8,&mut fs,b"/main"),Err(Error::Denied));
 }
}
