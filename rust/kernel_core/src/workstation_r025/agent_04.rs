//! R025/04: UTF-8-aware line/column source indexing for real project files.
//! File revision stamp prevents navigation into externally changed editor bytes.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Line{pub start:usize,pub end:usize}
pub const EMPTY:Line=Line{start:0,end:0};
pub struct Index<const L:usize>{lines:[Line;L],count:usize,revision:u64}
impl<const L:usize> Index<L>{
 pub const fn new()->Self{Self{lines:[EMPTY;L],count:0,revision:0}}
 pub fn build<const F:usize>(&mut self,fs:&RamFs<F>,path:&[u8])
  ->Result<usize,Error>{
  let (rev,len)=fs.metadata(path).map_err(|_|Error::Missing)?;
  let mut bytes=[0u8;CONTENT_MAX];
  fs.read(path,0,&mut bytes[..len]).map_err(|_|Error::Missing)?;
  core::str::from_utf8(&bytes[..len]).map_err(|_|Error::Invalid)?;
  let mut stage=[EMPTY;L];let mut n=0;let mut start=0;
  for (i,b) in bytes[..len].iter().enumerate(){
   if *b==b'\n'{
    if n>=L{return Err(Error::Full);}
    stage[n]=Line{start,end:i};n+=1;start=i+1;
   }
  }
  if start<len||len==0{
   if n>=L{return Err(Error::Full);}
   stage[n]=Line{start,end:len};n+=1;
  }
  self.lines=stage;self.count=n;self.revision=rev;Ok(n)
 }
 pub fn position<const F:usize>(&self,fs:&RamFs<F>,path:&[u8],
  line:usize,column:usize)->Result<usize,Error>{
  let (rev,len)=fs.metadata(path).map_err(|_|Error::Missing)?;
  if rev!=self.revision{return Err(Error::Stale);}
  if line==0||line>self.count||column==0{return Err(Error::Invalid);}
  let l=self.lines[line-1];let offset=l.start+column-1;
  if offset>l.end||offset>len{return Err(Error::Invalid);}
  let mut bytes=[0u8;CONTENT_MAX];
  fs.read(path,0,&mut bytes[..len]).map_err(|_|Error::Missing)?;
  if !core::str::from_utf8(&bytes[..len]).map_err(|_|Error::Invalid)?
   .is_char_boundary(offset){return Err(Error::Invalid);}
  Ok(offset)
 }
 pub fn line_at(&self,offset:usize)->Option<usize>{
  self.lines[..self.count].iter().position(|l|offset>=l.start&&offset<=l.end)
   .map(|i|i+1)
 }
 pub fn count(&self)->usize{self.count}
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn navigates_utf8_source_lines_to_exact_offsets(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"hello\n\xc3\xa9nd\n").unwrap();
  let mut index=Index::<4>::new();assert_eq!(index.build(&fs,b"/a"),Ok(2));
  assert_eq!(index.position(&fs,b"/a",2,1),Ok(6));
  assert_eq!(index.position(&fs,b"/a",2,2),Err(Error::Invalid));
 }
 #[test]fn editing_current_source_invalidates_previous_line_offsets(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"one\ntwo").unwrap();
  let mut i=Index::<2>::new();i.build(&fs,b"/a").unwrap();
  fs.replace(b"/a",b"changed").unwrap();
  assert_eq!(i.position(&fs,b"/a",1,1),Err(Error::Stale));
 }
 #[test]fn short_line_index_does_not_publish_partial_offsets(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"a\nb\nc").unwrap();
  let mut i=Index::<1>::new();
  assert_eq!(i.build(&fs,b"/a"),Err(Error::Full));
  assert_eq!(i.count(),0);
 }
}
