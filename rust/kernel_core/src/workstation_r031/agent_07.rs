//! R031/07: atomic, non-overlapping multiple-cursor editing of one real file.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX};
pub struct Edit<'a>{pub start:usize,pub end:usize,pub expected:&'a[u8],
 pub replacement:&'a[u8]}
pub fn apply<const F:usize>(fs:&mut RamFs<F>,path:&[u8],revision:u64,
 edits:&[Edit<'_>])->Result<u64,Error>{
 if edits.is_empty(){return Err(Error::Invalid);}
 let (actual,n)=fs.metadata(path).map_err(|_|Error::Missing)?;
 if actual!=revision{return Err(Error::Stale);}
 let mut bytes=[0u8;CONTENT_MAX];fs.read(path,0,&mut bytes[..n])
  .map_err(|_|Error::Missing)?;
 let s=core::str::from_utf8(&bytes[..n]).map_err(|_|Error::Invalid)?;
 let mut next=[0u8;CONTENT_MAX];let(mut cursor,mut w)=(0usize,0usize);
 for edit in edits{
  if edit.start<cursor||edit.start>edit.end||edit.end>n||
   !s.is_char_boundary(edit.start)||!s.is_char_boundary(edit.end)||
   &bytes[edit.start..edit.end]!=edit.expected||
   core::str::from_utf8(edit.replacement).is_err(){return Err(Error::Conflict);}
  let before=&bytes[cursor..edit.start];
  let required=w.checked_add(before.len()).and_then(|v|v.checked_add(edit.replacement.len()))
   .ok_or(Error::Overflow)?;
  if required>CONTENT_MAX{return Err(Error::Full);}
  next[w..w+before.len()].copy_from_slice(before);w+=before.len();
  next[w..required].copy_from_slice(edit.replacement);w=required;
  cursor=edit.end;
 }
 let remaining=n-cursor;
 if w+remaining>CONTENT_MAX{return Err(Error::Full);}
 next[w..w+remaining].copy_from_slice(&bytes[cursor..n]);w+=remaining;
 fs.replace(path,&next[..w]).map_err(|_|Error::Full)
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn two_separated_cursor_edits_apply_as_single_file_revision(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/p/a",b"one two three").unwrap();
  let edits=[Edit{start:0,end:3,expected:b"one",replacement:b"1"},
    Edit{start:8,end:13,expected:b"three",replacement:b"3"}];
  assert_eq!(apply(&mut fs,b"/p/a",1,&edits),Ok(2));
  let mut out=[0;7];fs.read(b"/p/a",0,&mut out).unwrap();assert_eq!(&out,b"1 two 3");
 }
 #[test]fn overlap_is_rejected_without_modifying_any_original_byte(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/p/a",b"abcdef").unwrap();
  let edits=[Edit{start:1,end:4,expected:b"bcd",replacement:b"x"},
   Edit{start:3,end:5,expected:b"de",replacement:b"y"}];
  assert_eq!(apply(&mut fs,b"/p/a",1,&edits),Err(Error::Conflict));
  assert_eq!(fs.version(),1);
 }
}