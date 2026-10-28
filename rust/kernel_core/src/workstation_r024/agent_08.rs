//! R024/08: UTF-8-safe editor selection, block copy and exact-preimage replacement.
//! Uses actual RamFs documents; rejects byte offsets within Unicode scalar boundaries.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Range{pub start:usize,pub end:usize,pub version:u64}
fn valid_boundary(s:&str,pos:usize)->bool{s.is_char_boundary(pos)}
pub fn select<const F:usize>(fs:&RamFs<F>,path:&[u8],a:usize,b:usize)
 ->Result<Range,Error>{
 let (version,len)=fs.metadata(path).map_err(|_|Error::Missing)?;
 let mut bytes=[0u8;CONTENT_MAX];
 fs.read(path,0,&mut bytes[..len]).map_err(|_|Error::Missing)?;
 let s=core::str::from_utf8(&bytes[..len]).map_err(|_|Error::Invalid)?;
 let start=a.min(b);let end=a.max(b);
 if end>len||!valid_boundary(s,start)||!valid_boundary(s,end){
  return Err(Error::Invalid);
 }
 Ok(Range{start,end,version})
}
pub fn copy<const F:usize>(fs:&RamFs<F>,path:&[u8],
 range:Range,out:&mut[u8])->Result<usize,Error>{
 let (version,len)=fs.metadata(path).map_err(|_|Error::Missing)?;
 if version!=range.version||range.end>len{return Err(Error::Stale);}
 let count=range.end-range.start;
 if out.len()<count{return Err(Error::Full);}
 fs.read(path,range.start as u64,&mut out[..count])
  .map_err(|_|Error::Missing)
}
pub fn replace<const F:usize>(fs:&mut RamFs<F>,path:&[u8],
 range:Range,bytes:&[u8])->Result<u64,Error>{
 let (version,size)=fs.metadata(path).map_err(|_|Error::Missing)?;
 if version!=range.version||range.end>size{return Err(Error::Stale);}
 if core::str::from_utf8(bytes).is_err(){return Err(Error::Invalid);}
 let mut source=[0u8;CONTENT_MAX];
 fs.read(path,0,&mut source[..size]).map_err(|_|Error::Missing)?;
 let s=core::str::from_utf8(&source[..size]).map_err(|_|Error::Invalid)?;
 if !s.is_char_boundary(range.start)||!s.is_char_boundary(range.end){
  return Err(Error::Invalid);
 }
 let next=size-(range.end-range.start)+bytes.len();
 if next>CONTENT_MAX{return Err(Error::Full);}
 let mut output=[0u8;CONTENT_MAX];
 output[..range.start].copy_from_slice(&source[..range.start]);
 output[range.start..range.start+bytes.len()].copy_from_slice(bytes);
 output[range.start+bytes.len()..next].copy_from_slice(&source[range.end..size]);
 fs.replace(path,&output[..next]).map_err(|_|Error::Full)
}
pub fn length(range:Range)->usize{range.end-range.start}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn unicode_safe_source_selection_and_replacement(){
  let mut fs=RamFs::<1>::new();
  fs.replace(b"/a",b"Hello \xc3\xa9 world").unwrap();
  let range=select(&fs,b"/a",6,8).unwrap();
  let mut out=[0;2];assert_eq!(copy(&fs,b"/a",range,&mut out),Ok(2));
  assert_eq!(out,[0xc3,0xa9]);replace(&mut fs,b"/a",range,b"E").unwrap();
  let mut text=[0;32];let n=fs.read(b"/a",0,&mut text).unwrap();
  assert_eq!(&text[..n],b"Hello E world");
 }
 #[test]fn splitting_multibyte_utf8_character_is_denied(){
  let mut fs=RamFs::<1>::new();
  fs.replace(b"/a",b"\xc3\xa9").unwrap();
  assert_eq!(select(&fs,b"/a",1,2),Err(Error::Invalid));
 }
 #[test]fn concurrent_edit_blocks_stale_editor_selection(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"foo").unwrap();
  let range=select(&fs,b"/a",0,3).unwrap();
  fs.replace(b"/a",b"bar").unwrap();
  assert_eq!(replace(&mut fs,b"/a",range,b"new"),Err(Error::Stale));
 }
}
