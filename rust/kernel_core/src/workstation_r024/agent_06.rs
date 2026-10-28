//! R024/06: byte-exact project source diff with first/last change and hunk ranges.
//! Reads actual RamFs bytes and does not fabricate a Git history.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Delta{pub old_start:usize,pub old_end:usize,pub new_start:usize,
 pub new_end:usize,pub shared_prefix:usize,pub shared_suffix:usize}
pub fn compare(old:&[u8],new:&[u8])->Delta{
 let prefix=old.iter().zip(new).take_while(|(a,b)|a==b).count();
 let suffix=old[prefix..].iter().rev().zip(new[prefix..].iter().rev())
  .take_while(|(a,b)|a==b).count();
 Delta{old_start:prefix,old_end:old.len()-suffix,new_start:prefix,
  new_end:new.len()-suffix,shared_prefix:prefix,shared_suffix:suffix}
}
pub fn files<const F:usize>(fs:&RamFs<F>,a:&[u8],b:&[u8])
 ->Result<Delta,Error>{
 let (_,size_a)=fs.metadata(a).map_err(|_|Error::Missing)?;
 let (_,size_b)=fs.metadata(b).map_err(|_|Error::Missing)?;
 let mut old=[0u8;CONTENT_MAX];let mut new=[0u8;CONTENT_MAX];
 fs.read(a,0,&mut old[..size_a]).map_err(|_|Error::Missing)?;
 fs.read(b,0,&mut new[..size_b]).map_err(|_|Error::Missing)?;
 Ok(compare(&old[..size_a],&new[..size_b]))
}
pub fn preview<const F:usize>(fs:&RamFs<F>,a:&[u8],b:&[u8],
 old_out:&mut[u8],new_out:&mut[u8])->Result<Delta,Error>{
 let d=files(fs,a,b)?;
 let (_,len_a)=fs.metadata(a).map_err(|_|Error::Missing)?;
 let (_,len_b)=fs.metadata(b).map_err(|_|Error::Missing)?;
 if d.old_end>len_a||d.new_end>len_b||old_out.len()<d.old_end-d.old_start||
  new_out.len()<d.new_end-d.new_start{return Err(Error::Full);}
 let mut old=[0u8;CONTENT_MAX];let mut new=[0u8;CONTENT_MAX];
 fs.read(a,0,&mut old[..len_a]).map_err(|_|Error::Missing)?;
 fs.read(b,0,&mut new[..len_b]).map_err(|_|Error::Missing)?;
 old_out[..d.old_end-d.old_start].copy_from_slice(&old[d.old_start..d.old_end]);
 new_out[..d.new_end-d.new_start].copy_from_slice(&new[d.new_start..d.new_end]);
 Ok(d)
}
pub fn identical(delta:Delta)->bool{
 delta.old_start==delta.old_end&&delta.new_start==delta.new_end
}
pub fn byte_delta(delta:Delta)->isize{
 (delta.new_end-delta.new_start) as isize-
 (delta.old_end-delta.old_start) as isize
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn actual_two_source_file_diff_returns_smallest_changed_span(){
  let mut fs=RamFs::<2>::new();
  fs.replace(b"/old",b"hello world").unwrap();
  fs.replace(b"/new",b"hello rust").unwrap();
  let d=files(&fs,b"/old",b"/new").unwrap();
  assert_eq!(d.shared_prefix,6);
  let mut o=[0;12];let mut n=[0;12];
  preview(&fs,b"/old",b"/new",&mut o,&mut n).unwrap();
  assert_eq!(&o[..5],b"world");assert_eq!(&n[..4],b"rust");
 }
 #[test]fn identical_sources_have_empty_diff(){
  let d=compare(b"source",b"source");assert!(identical(d));assert_eq!(byte_delta(d),0);
 }
 #[test]fn insufficient_diff_preview_does_not_modify_output(){
  let mut fs=RamFs::<2>::new();
  fs.replace(b"/a",b"old").unwrap();fs.replace(b"/b",b"new").unwrap();
  let mut out=[99;1];let mut new=[77;3];
  assert_eq!(preview(&fs,b"/a",b"/b",&mut out,&mut new),Err(Error::Full));
  assert_eq!(out,[99]);assert_eq!(new,[77;3]);
 }
}
