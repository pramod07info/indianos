//! R022/08: exact-preimage multi-cursor text edits in a single real file.
//! Descending, non-overlapping edit ranges are applied atomically in a RAM staging buffer.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX};
pub struct Edit<'a>{pub start:usize,pub old:&'a[u8],pub replace:&'a[u8]}
pub fn apply<const F:usize>(fs:&mut RamFs<F>,path:&[u8],
 expected_file_rev:u64,changes:&[Edit<'_>])->Result<u64,Error>{
 if changes.is_empty(){return Err(Error::Invalid);}
 let (version,len)=fs.metadata(path).map_err(|_|Error::Missing)?;
 if version!=expected_file_rev{return Err(Error::Stale);}
 let mut data=[0u8;CONTENT_MAX];
 fs.read(path,0,&mut data[..len]).map_err(|_|Error::Missing)?;
 let mut out=[0u8;CONTENT_MAX];
 let mut cursor=0usize;let mut written=0usize;
 for (i,edit) in changes.iter().enumerate(){
  let end=edit.start.checked_add(edit.old.len()).ok_or(Error::Overflow)?;
  if edit.start<cursor||end>len||edit.old.is_empty()||
   &data[edit.start..end]!=edit.old||
   (i>0&&changes[i-1].start>=edit.start){return Err(Error::Conflict);}
  let segment=edit.start-cursor;
  let next=written.checked_add(segment).and_then(|n|n.checked_add(edit.replace.len()))
   .ok_or(Error::Overflow)?;
  if next>CONTENT_MAX{return Err(Error::Full);}
  out[written..written+segment].copy_from_slice(&data[cursor..edit.start]);
  written+=segment;
  out[written..next].copy_from_slice(edit.replace);
  written=next;cursor=end;
 }
 let remain=len-cursor;
 let end=written.checked_add(remain).ok_or(Error::Overflow)?;
 if end>CONTENT_MAX{return Err(Error::Full);}
 out[written..end].copy_from_slice(&data[cursor..len]);
 // No changes to live file until every edit and final output are validated.
 fs.replace(path,&out[..end]).map_err(|_|Error::Full)
}
pub fn count_changes(changes:&[Edit<'_>])->usize{changes.len()}
pub fn net_bytes(changes:&[Edit<'_>])->Result<isize,Error>{
 let mut delta=0isize;
 for change in changes{
  delta=delta.checked_add(change.replace.len() as isize-
    change.old.len() as isize).ok_or(Error::Overflow)?;
 }Ok(delta)
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn multi_cursor_replacement_edits_two_locations_in_real_file(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/main",b"foo + foo").unwrap();
  let changes=[Edit{start:0,old:b"foo",replace:b"bar"},
   Edit{start:6,old:b"foo",replace:b"bar"}];
  apply(&mut fs,b"/main",1,&changes).unwrap();
  let mut out=[0;9];fs.read(b"/main",0,&mut out).unwrap();
  assert_eq!(&out,b"bar + bar");assert_eq!(net_bytes(&changes),Ok(0));
 }
 #[test]fn second_invalid_preimage_cannot_publish_first_edit(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"foo foo").unwrap();
  let changes=[Edit{start:0,old:b"foo",replace:b"bar"},
   Edit{start:4,old:b"bad",replace:b"new"}];
  assert_eq!(apply(&mut fs,b"/a",1,&changes),Err(Error::Conflict));
  let mut out=[0;7];fs.read(b"/a",0,&mut out).unwrap();assert_eq!(&out,b"foo foo");
 }
 #[test]fn overlapping_source_edits_are_rejected_without_lost_changes(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"abcdef").unwrap();
  let changes=[Edit{start:0,old:b"abc",replace:b"x"},
   Edit{start:2,old:b"cde",replace:b"y"}];
  assert_eq!(apply(&mut fs,b"/a",1,&changes),Err(Error::Conflict));
  assert_eq!(fs.version(),1);
 }
}
