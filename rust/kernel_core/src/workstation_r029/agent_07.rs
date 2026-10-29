//! R029/07: owner-bound UTF-8 source clipboard, selection and safe paste to real files.
use super::Error;
use crate::batch5_session::Identity;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX};
pub struct Clipboard{owner:Identity,bytes:[u8;CONTENT_MAX],len:usize}
impl Clipboard{
 pub const fn new(owner:Identity)->Self{Self{owner,bytes:[0;CONTENT_MAX],len:0}}
 fn auth(&self,who:Identity)->Result<(),Error>{
  if who.pid==0||who!=self.owner{return Err(Error::Denied);}Ok(())
 }
 pub fn copy(&mut self,who:Identity,source:&[u8],
 start:usize,end:usize)->Result<(),Error>{
  self.auth(who)?;
  let s=core::str::from_utf8(source).map_err(|_|Error::Invalid)?;
  if end>source.len()||start>end||!s.is_char_boundary(start)||
   !s.is_char_boundary(end){return Err(Error::Invalid);}
  let len=end-start;if len>CONTENT_MAX{return Err(Error::Full);}
  self.bytes[..len].copy_from_slice(&source[start..end]);self.len=len;Ok(())
 }
 pub fn paste<const F:usize>(&self,who:Identity,fs:&mut RamFs<F>,path:&[u8],
 revision:u64,offset:usize)->Result<u64,Error>{
  self.auth(who)?;
  let (rev,n)=fs.metadata(path).map_err(|_|Error::Missing)?;
  if rev!=revision{return Err(Error::Stale);}
  let mut original=[0u8;CONTENT_MAX];
  fs.read(path,0,&mut original[..n]).map_err(|_|Error::Missing)?;
  let s=core::str::from_utf8(&original[..n]).map_err(|_|Error::Invalid)?;
  if !s.is_char_boundary(offset)||n+self.len>CONTENT_MAX{return Err(Error::Invalid);}
  let mut out=[0u8;CONTENT_MAX];
  out[..offset].copy_from_slice(&original[..offset]);
  out[offset..offset+self.len].copy_from_slice(&self.bytes[..self.len]);
  out[offset+self.len..n+self.len].copy_from_slice(&original[offset..n]);
  fs.replace(path,&out[..n+self.len]).map_err(|_|Error::Full)
 }
 pub fn clear(&mut self,who:Identity)->Result<(),Error>{
  self.auth(who)?;self.bytes=[0;CONTENT_MAX];self.len=0;Ok(())
 }
 pub fn len(&self,who:Identity)->Result<usize,Error>{self.auth(who)?;Ok(self.len)}
}
#[cfg(test)]mod tests{
 use super::*;fn id(g:u64)->Identity{Identity{pid:7,generation:g}}
 #[test]fn copying_utf8_selection_pastes_into_live_source_file(){
  let mut clip=Clipboard::new(id(1));clip.copy(id(1),"é-hello".as_bytes(),0,2).unwrap();
  let mut fs=RamFs::<1>::new();fs.replace(b"/main",b"fn main").unwrap();
  clip.paste(id(1),&mut fs,b"/main",1,3).unwrap();
  let mut out=[0;32];let n=fs.read(b"/main",0,&mut out).unwrap();
  assert_eq!(&out[..n],"fn émain".as_bytes());
 }
 #[test]fn other_generation_cannot_paste_users_clipboard(){
  let mut clip=Clipboard::new(id(1));clip.copy(id(1),b"test",0,4).unwrap();
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"old").unwrap();
  assert_eq!(clip.paste(id(2),&mut fs,b"/a",1,0),Err(Error::Denied));
 }
}