//! R030/06: owner-scoped multi-document dirty edit buffers, saved with revision guards.
use super::Error;
use crate::batch5_session::Identity;
use crate::batch7_ramfs::{RamFs,PATH_MAX,CONTENT_MAX};
#[derive(Clone,Copy)]
pub struct Buffer{pub path:[u8;PATH_MAX],pub len:usize,pub revision:u64,
 pub data:[u8;CONTENT_MAX],pub size:usize,pub dirty:bool}
pub struct Editor<const N:usize>{owner:Identity,tabs:[Option<Buffer>;N]}
impl<const N:usize> Editor<N>{
 pub const fn new(owner:Identity)->Self{Self{owner,tabs:[None;N]}}
 fn auth(&self,who:Identity)->Result<(),Error>{
  if who.pid==0||who!=self.owner{return Err(Error::Denied);}Ok(())
 }
 fn index(&self,path:&[u8])->Option<usize>{
  self.tabs.iter().position(|t|t.is_some_and(|x|&x.path[..x.len]==path))
 }
 pub fn open<const F:usize>(&mut self,who:Identity,fs:&RamFs<F>,
 path:&[u8])->Result<(),Error>{
  self.auth(who)?;
  if path.len()<2||path.len()>PATH_MAX||self.index(path).is_some(){return Err(Error::Invalid);}
  let(revision,size)=fs.metadata(path).map_err(|_|Error::Missing)?;
  let mut buf=Buffer{path:[0;PATH_MAX],len:path.len(),revision,
   data:[0;CONTENT_MAX],size,dirty:false};
  buf.path[..path.len()].copy_from_slice(path);
  fs.read(path,0,&mut buf.data[..size]).map_err(|_|Error::Missing)?;
  let slot=self.tabs.iter().position(Option::is_none).ok_or(Error::Full)?;
  self.tabs[slot]=Some(buf);Ok(())
 }
 pub fn edit(&mut self,who:Identity,path:&[u8],text:&[u8])->Result<(),Error>{
  self.auth(who)?;
  if text.len()>CONTENT_MAX||core::str::from_utf8(text).is_err(){
   return Err(Error::Invalid);
  }
  let i=self.index(path).ok_or(Error::Missing)?;
  let mut b=self.tabs[i].ok_or(Error::Missing)?;
  b.data=[0;CONTENT_MAX];b.data[..text.len()].copy_from_slice(text);
  b.size=text.len();b.dirty=true;self.tabs[i]=Some(b);Ok(())
 }
 pub fn save<const F:usize>(&mut self,who:Identity,fs:&mut RamFs<F>,
 path:&[u8])->Result<u64,Error>{
  self.auth(who)?;
  let i=self.index(path).ok_or(Error::Missing)?;
  let mut b=self.tabs[i].ok_or(Error::Missing)?;
  let(rev,_)=fs.metadata(path).map_err(|_|Error::Missing)?;
  if rev!=b.revision{return Err(Error::Stale);}
  if !b.dirty{return Ok(fs.version());}
  let next=fs.replace(path,&b.data[..b.size]).map_err(|_|Error::Full)?;
  b.revision=next;b.dirty=false;self.tabs[i]=Some(b);Ok(next)
 }
 pub fn get(&self,who:Identity,path:&[u8])->Result<Buffer,Error>{
  self.auth(who)?;self.index(path).and_then(|i|self.tabs[i]).ok_or(Error::Missing)
 }
 pub fn close(&mut self,who:Identity,path:&[u8],discard:bool)->Result<(),Error>{
  self.auth(who)?;let i=self.index(path).ok_or(Error::Missing)?;
  if self.tabs[i].is_some_and(|t|t.dirty)&&!discard{return Err(Error::Conflict);}
  self.tabs[i]=None;Ok(())
 }
}
#[cfg(test)]mod tests{
 use super::*;fn id(g:u64)->Identity{Identity{pid:4,generation:g}}
 #[test]fn real_dirty_source_buffer_saves_only_if_file_version_matches(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/main",b"old").unwrap();
  let mut editor=Editor::<1>::new(id(1));editor.open(id(1),&fs,b"/main").unwrap();
  editor.edit(id(1),b"/main",b"edited").unwrap();
  assert_eq!(editor.close(id(1),b"/main",false),Err(Error::Conflict));
  editor.save(id(1),&mut fs,b"/main").unwrap();
  let mut output=[0;6];fs.read(b"/main",0,&mut output).unwrap();
  assert_eq!(&output,b"edited");
 }
 #[test]fn outside_edit_prevents_stale_document_save_and_generation_leak(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/main",b"old").unwrap();
  let mut e=Editor::<1>::new(id(1));e.open(id(1),&fs,b"/main").unwrap();
  e.edit(id(1),b"/main",b"draft").unwrap();fs.replace(b"/main",b"new").unwrap();
  assert_eq!(e.save(id(1),&mut fs,b"/main"),Err(Error::Stale));
  assert!(matches!(e.get(id(2),b"/main"),Err(Error::Denied)));
 }
}