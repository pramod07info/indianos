//! R032/09: UTF-8 character safe editor selection with stale revision protection.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX,PATH_MAX};
pub struct Selection{owner:u64,path:[u8;PATH_MAX],len:usize,
 revision:u64,start:usize,end:usize}
impl Selection{
 pub fn select<const F:usize>(owner:u64,fs:&RamFs<F>,
  path:&[u8],start:usize,end:usize)->Result<Self,Error>{
  if owner==0||path.len()>PATH_MAX||start>end{return Err(Error::Invalid);}
  let(rev,n)=fs.metadata(path).map_err(|_|Error::Missing)?;
  if end>n{return Err(Error::Invalid);}
  let mut buf=[0u8;CONTENT_MAX];fs.read(path,0,&mut buf[..n])
   .map_err(|_|Error::Missing)?;
  let text=core::str::from_utf8(&buf[..n]).map_err(|_|Error::Invalid)?;
  if !text.is_char_boundary(start)||!text.is_char_boundary(end){
   return Err(Error::Invalid);
  }
  let mut name=[0u8;PATH_MAX];name[..path.len()].copy_from_slice(path);
  Ok(Self{owner,path:name,len:path.len(),revision:rev,start,end})
 }
 pub fn copy<const F:usize>(&self,owner:u64,fs:&RamFs<F>,
  out:&mut[u8])->Result<usize,Error>{
  if owner==0||owner!=self.owner{return Err(Error::Denied);}
  let path=&self.path[..self.len];
  let(rev,_)=fs.metadata(path).map_err(|_|Error::Missing)?;
  if rev!=self.revision{return Err(Error::Stale);}
  let count=self.end-self.start;
  if count>out.len(){return Err(Error::Full);}
  let n=fs.read(path,self.start as u64,&mut out[..count])
   .map_err(|_|Error::Missing)?;
  if n!=count{return Err(Error::Conflict);}Ok(n)
 }
 pub fn range(&self,owner:u64)->Result<(usize,usize),Error>{
  if owner!=self.owner||owner==0{return Err(Error::Denied);}
  Ok((self.start,self.end))
 }
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn selection_copies_whole_unicode_character_without_breaking_utf8(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/p/a","αβz".as_bytes()).unwrap();
  let s=Selection::select(7,&fs,b"/p/a",2,4).unwrap();
  let mut out=[0;2];assert_eq!(s.copy(7,&fs,&mut out),Ok(2));
  assert_eq!(&out,"β".as_bytes());
 }
 #[test]fn stale_document_and_partial_unicode_selection_fail(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/p/a","αβ".as_bytes()).unwrap();
  assert!(Selection::select(7,&fs,b"/p/a",1,2).is_err());
  let s=Selection::select(7,&fs,b"/p/a",0,2).unwrap();
  fs.replace(b"/p/a",b"new").unwrap();
  assert_eq!(s.copy(7,&fs,&mut[0;4]),Err(Error::Stale));
 }
}