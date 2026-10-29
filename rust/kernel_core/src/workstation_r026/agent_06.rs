//! R026/06: UTF-8 safe simultaneous multi-cursor edits on real files.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Selection{pub start:usize,pub end:usize}
pub fn transform<const B:usize>(source:&[u8],sel:&[Selection],replacement:&[u8],
 out:&mut[u8;B])->Result<usize,Error>{
 let s=core::str::from_utf8(source).map_err(|_|Error::Invalid)?;
 if sel.is_empty()||core::str::from_utf8(replacement).is_err(){return Err(Error::Invalid);}
 let mut last=0;let mut size=source.len();
 for r in sel{
  if r.start<last||r.end<r.start||r.end>source.len()||
   !s.is_char_boundary(r.start)||!s.is_char_boundary(r.end){return Err(Error::Invalid);}
  size=size.checked_sub(r.end-r.start)
   .and_then(|n|n.checked_add(replacement.len())).ok_or(Error::Overflow)?;
  last=r.end;
 }
 if size>B{return Err(Error::Full);}
 let mut stage=[0u8;B];let mut pos=0;last=0;
 for r in sel{
  let text=&source[last..r.start];
  stage[pos..pos+text.len()].copy_from_slice(text);pos+=text.len();
  stage[pos..pos+replacement.len()].copy_from_slice(replacement);
  pos+=replacement.len();last=r.end;
 }
 let tail=&source[last..];stage[pos..pos+tail.len()].copy_from_slice(tail);
 pos+=tail.len();*out=stage;Ok(pos)
}
pub fn apply<const F:usize>(fs:&mut RamFs<F>,path:&[u8],expected:u64,
 sel:&[Selection],replacement:&[u8])->Result<u64,Error>{
 let (rev,size)=fs.metadata(path).map_err(|_|Error::Missing)?;
 if rev!=expected{return Err(Error::Stale);}
 let mut source=[0u8;CONTENT_MAX];fs.read(path,0,&mut source[..size])
  .map_err(|_|Error::Missing)?;
 let mut next=[0u8;CONTENT_MAX];
 let n=transform(&source[..size],sel,replacement,&mut next)?;
 fs.replace(path,&next[..n]).map_err(|_|Error::Full)
}
pub fn selected_bytes(sel:&[Selection])->usize{
 sel.iter().map(|x|x.end.saturating_sub(x.start)).sum()
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn simultaneous_two_cursor_change_on_actual_source_file(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"one two one").unwrap();
  apply(&mut fs,b"/a",1,&[Selection{start:0,end:3},Selection{start:8,end:11}],b"X").unwrap();
  let mut out=[0;32];let n=fs.read(b"/a",0,&mut out).unwrap();
  assert_eq!(&out[..n],b"X two X");
 }
 #[test]fn overlapping_ranges_rejected_atomically(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"abcdef").unwrap();
  assert_eq!(apply(&mut fs,b"/a",1,&[Selection{start:0,end:3},
   Selection{start:2,end:4}],b"x"),Err(Error::Invalid));
  assert_eq!(fs.version(),1);
 }
 #[test]fn multibyte_split_and_stale_selection_fail_closed(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"\xc3\xa9").unwrap();
  assert_eq!(apply(&mut fs,b"/a",1,&[Selection{start:1,end:2}],b"x"),Err(Error::Invalid));
  fs.replace(b"/a",b"x").unwrap();
  assert_eq!(apply(&mut fs,b"/a",1,&[Selection{start:0,end:1}],b"z"),Err(Error::Stale));
 }
}
