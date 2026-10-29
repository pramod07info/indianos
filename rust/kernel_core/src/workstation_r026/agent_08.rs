//! R026/08: owned UTF-8 editor snippet templates with real-file insertion.
use super::Error;
use crate::batch5_session::Identity;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX};
pub const LIMIT:usize=256;
#[derive(Clone,Copy)]
pub struct Snippet{pub id:u32,pub bytes:[u8;LIMIT],pub len:usize}
pub struct Library<const N:usize>{owner:Identity,items:[Option<Snippet>;N]}
impl<const N:usize> Library<N>{
 pub const fn new(owner:Identity)->Self{Self{owner,items:[None;N]}}
 fn check(&self,actor:Identity)->Result<(),Error>{
  if actor.pid==0||actor!=self.owner{return Err(Error::Denied);}Ok(())
 }
 fn index(&self,id:u32)->Option<usize>{
  self.items.iter().position(|x|x.is_some_and(|v|v.id==id))
 }
 pub fn add(&mut self,actor:Identity,id:u32,text:&[u8])->Result<(),Error>{
  self.check(actor)?;
  if id==0||text.is_empty()||text.len()>LIMIT||
   core::str::from_utf8(text).is_err()||self.index(id).is_some(){
   return Err(Error::Invalid);
  }
  let at=self.items.iter().position(Option::is_none).ok_or(Error::Full)?;
  let mut item=Snippet{id,bytes:[0;LIMIT],len:text.len()};
  item.bytes[..text.len()].copy_from_slice(text);self.items[at]=Some(item);Ok(())
 }
 pub fn insert<const F:usize>(&self,actor:Identity,fs:&mut RamFs<F>,
 id:u32,path:&[u8],expected:u64,offset:usize)->Result<u64,Error>{
  self.check(actor)?;
  let item=self.index(id).and_then(|i|self.items[i]).ok_or(Error::Missing)?;
  let (revision,len)=fs.metadata(path).map_err(|_|Error::Missing)?;
  if revision!=expected||offset>len{return Err(Error::Stale);}
  let size=len.checked_add(item.len).ok_or(Error::Overflow)?;
  if size>CONTENT_MAX{return Err(Error::Full);}
  let mut source=[0u8;CONTENT_MAX];
  fs.read(path,0,&mut source[..len]).map_err(|_|Error::Missing)?;
  if !core::str::from_utf8(&source[..len]).map_err(|_|Error::Invalid)?
   .is_char_boundary(offset){return Err(Error::Invalid);}
  let mut output=[0u8;CONTENT_MAX];
  output[..offset].copy_from_slice(&source[..offset]);
  output[offset..offset+item.len].copy_from_slice(&item.bytes[..item.len]);
  output[offset+item.len..size].copy_from_slice(&source[offset..len]);
  fs.replace(path,&output[..size]).map_err(|_|Error::Full)
 }
 pub fn delete(&mut self,actor:Identity,id:u32)->Result<(),Error>{
  self.check(actor)?;let i=self.index(id).ok_or(Error::Missing)?;
  self.items[i]=None;Ok(())
 }
 pub fn count(&self,actor:Identity)->Result<usize,Error>{
  self.check(actor)?;Ok(self.items.iter().flatten().count())
 }
}
#[cfg(test)]mod tests{
 use super::*;fn id(g:u64)->Identity{Identity{pid:7,generation:g}}
 #[test]fn inserts_literal_snippet_into_guest_source(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/main",b"fn main(){}").unwrap();
  let mut l=Library::<1>::new(id(1));l.add(id(1),1,b"pub ").unwrap();
  l.insert(id(1),&mut fs,1,b"/main",1,0).unwrap();
  let mut out=[0u8;15];fs.read(b"/main",0,&mut out).unwrap();
  assert_eq!(&out,b"pub fn main(){}");
 }
 #[test]fn process_generation_reuse_cannot_inject_private_template(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/main",b"main").unwrap();
  let mut l=Library::<1>::new(id(1));l.add(id(1),1,b"x").unwrap();
  assert_eq!(l.insert(id(2),&mut fs,1,b"/main",1,0),Err(Error::Denied));
 }
 #[test]fn reject_oversized_snippet_and_stale_file(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/main",b"main").unwrap();
  let mut l=Library::<1>::new(id(1));
  assert_eq!(l.add(id(1),1,&[1;LIMIT+1]),Err(Error::Invalid));
  l.add(id(1),1,b"text").unwrap();fs.replace(b"/main",b"new").unwrap();
  assert_eq!(l.insert(id(1),&mut fs,1,b"/main",1,0),Err(Error::Stale));
 }
}
