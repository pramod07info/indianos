//! R028/05: revision-conscious source diff with bounded changed line report.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Summary{pub added:usize,pub removed:usize,pub modified:usize,
 pub old_lines:usize,pub new_lines:usize}
fn lines<'a>(bytes:&'a[u8],out:&mut[&'a[u8];256])->Result<usize,Error>{
 let mut count=0;
 for line in bytes.split_inclusive(|b|*b==b'\n'){
  if line.is_empty(){continue;}
  if count==out.len(){return Err(Error::Full);}
  out[count]=line;count+=1;
 }Ok(count)
}
pub fn compare(old:&[u8],new:&[u8])->Result<Summary,Error>{
 let mut before=[&b""[..];256];let mut after=[&b""[..];256];
 let a=lines(old,&mut before)?;let b=lines(new,&mut after)?;
 let mut modified=0;
 for i in 0..a.min(b){if before[i]!=after[i]{modified+=1;}}
 Ok(Summary{added:b.saturating_sub(a),removed:a.saturating_sub(b),
  modified,old_lines:a,new_lines:b})
}
pub fn files<const F:usize>(fs:&RamFs<F>,before:&[u8],after:&[u8])
 ->Result<Summary,Error>{
 let (_,a)=fs.metadata(before).map_err(|_|Error::Missing)?;
 let (_,b)=fs.metadata(after).map_err(|_|Error::Missing)?;
 let mut x=[0u8;CONTENT_MAX];let mut y=[0u8;CONTENT_MAX];
 fs.read(before,0,&mut x[..a]).map_err(|_|Error::Missing)?;
 fs.read(after,0,&mut y[..b]).map_err(|_|Error::Missing)?;
 compare(&x[..a],&y[..b])
}
pub fn changed(s:Summary)->usize{s.added+s.removed+s.modified}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn real_two_source_file_diff_counts_changed_and_added_lines(){
  let mut fs=RamFs::<2>::new();
  fs.replace(b"/a",b"one\ntwo\n").unwrap();
  fs.replace(b"/b",b"one\nTHREE\nfour\n").unwrap();
  let result=files(&fs,b"/a",b"/b").unwrap();
  assert_eq!((result.modified,result.added),(1,1));
 }
 #[test]fn no_change_report_is_zero_and_binary_empty_is_supported(){
  assert_eq!(changed(compare(b"abc",b"abc").unwrap()),0);
  assert_eq!(changed(compare(b"",b"").unwrap()),0);
 }
}