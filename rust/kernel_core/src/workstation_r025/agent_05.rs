//! R025/05: revision-aware editor search with bounded pagination and next-hit nav.
//! Finds literal source matches in actual RamFs file content, no fuzzy guessing.
use super::Error;
use crate::batch5_session::Identity;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Hit{pub offset:usize,pub line:usize}
pub const EMPTY:Hit=Hit{offset:0,line:0};
pub struct Search<const N:usize>{owner:Identity,hits:[Hit;N],len:usize,
 revision:u64,cursor:usize}
impl<const N:usize> Search<N>{
 pub const fn new(owner:Identity)->Self{Self{owner,hits:[EMPTY;N],len:0,
  revision:0,cursor:0}}
 fn auth(&self,actor:Identity)->Result<(),Error>{
  if actor.pid==0||actor!=self.owner{return Err(Error::Denied);}Ok(())
 }
 pub fn refresh<const F:usize>(&mut self,actor:Identity,fs:&RamFs<F>,
  path:&[u8],needle:&[u8])->Result<usize,Error>{
  self.auth(actor)?;
  if needle.is_empty()||needle.len()>64{return Err(Error::Invalid);}
  let (rev,size)=fs.metadata(path).map_err(|_|Error::Missing)?;
  let mut bytes=[0u8;CONTENT_MAX];fs.read(path,0,&mut bytes[..size])
   .map_err(|_|Error::Missing)?;
  let mut stage=[EMPTY;N];let mut count=0;let mut line=1usize;
  if needle.len()<=size{
   for i in 0..=size-needle.len(){
    if &bytes[i..i+needle.len()]==needle{
     if count==N{return Err(Error::Full);}
     stage[count]=Hit{offset:i,line};count+=1;
    }
    if bytes[i]==b'\n'{line+=1;}
   }
  }
  self.hits=stage;self.len=count;self.revision=rev;self.cursor=0;Ok(count)
 }
 pub fn next<const F:usize>(&mut self,actor:Identity,fs:&RamFs<F>,
  path:&[u8])->Result<Hit,Error>{
  self.auth(actor)?;
  if self.len==0{return Err(Error::Missing);}
  if fs.metadata(path).map_err(|_|Error::Missing)?.0!=self.revision{
   return Err(Error::Stale);
  }
  let item=self.hits[self.cursor%self.len];self.cursor=(self.cursor+1)%self.len;
  Ok(item)
 }
 pub fn page(&self,actor:Identity,start:usize,
  out:&mut[Hit])->Result<usize,Error>{
  self.auth(actor)?;
  if start>self.len{return Err(Error::Invalid);}
  let count=(self.len-start).min(out.len());
  out[..count].copy_from_slice(&self.hits[start..start+count]);Ok(count)
 }
 pub fn count(&self,actor:Identity)->Result<usize,Error>{
  self.auth(actor)?;Ok(self.len)
 }
}
#[cfg(test)]mod tests{
 use super::*;fn id(g:u64)->Identity{Identity{pid:7,generation:g}}
 #[test]fn editor_search_finds_lines_and_wraps_to_first_hit(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"foo\nx foo\n").unwrap();
  let mut s=Search::<3>::new(id(1));assert_eq!(s.refresh(id(1),&fs,b"/a",b"foo"),Ok(2));
  assert_eq!(s.next(id(1),&fs,b"/a").unwrap().line,1);
  assert_eq!(s.next(id(1),&fs,b"/a").unwrap().line,2);
  assert_eq!(s.next(id(1),&fs,b"/a").unwrap().line,1);
 }
 #[test]fn stale_source_prevents_jump_to_old_cursor(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"foo").unwrap();
  let mut s=Search::<1>::new(id(1));s.refresh(id(1),&fs,b"/a",b"foo").unwrap();
  fs.replace(b"/a",b"bar").unwrap();
  assert_eq!(s.next(id(1),&fs,b"/a"),Err(Error::Stale));
 }
 #[test]fn too_many_hits_does_not_publish_partial_index(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"aaaa").unwrap();
  let mut s=Search::<1>::new(id(1));
  assert_eq!(s.refresh(id(1),&fs,b"/a",b"a"),Err(Error::Full));
  assert_eq!(s.count(id(1)),Ok(0));
 }
}
