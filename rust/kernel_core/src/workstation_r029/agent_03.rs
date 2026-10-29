//! R029/03: bounded byte-level file comparison with source offset windows.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Range{pub offset:usize,pub changed:usize}
pub const EMPTY:Range=Range{offset:0,changed:0};
pub fn compare<const N:usize>(left:&[u8],right:&[u8],
 out:&mut[Range;N])->Result<usize,Error>{
 let mut ranges=[EMPTY;N];let mut count=0;let mut at=0;
 while at<left.len().max(right.len()){
  if left.get(at)==right.get(at){at+=1;continue;}
  let start=at;
  while at<left.len().max(right.len())&&left.get(at)!=right.get(at){at+=1;}
  if count==N{return Err(Error::Full);}
  ranges[count]=Range{offset:start,changed:at-start};count+=1;
 }
 *out=ranges;Ok(count)
}
pub fn files<const F:usize,const N:usize>(fs:&RamFs<F>,a:&[u8],b:&[u8],
 out:&mut[Range;N])->Result<usize,Error>{
 let (_,a_len)=fs.metadata(a).map_err(|_|Error::Missing)?;
 let (_,b_len)=fs.metadata(b).map_err(|_|Error::Missing)?;
 let mut first=[0u8;CONTENT_MAX];let mut second=[0u8;CONTENT_MAX];
 fs.read(a,0,&mut first[..a_len]).map_err(|_|Error::Missing)?;
 fs.read(b,0,&mut second[..b_len]).map_err(|_|Error::Missing)?;
 compare(&first[..a_len],&second[..b_len],out)
}
pub fn changed_bytes(changes:&[Range])->usize{changes.iter().map(|x|x.changed).sum()}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn byte_diff_reports_two_changed_regions_from_real_files(){
  let mut fs=RamFs::<2>::new();fs.replace(b"/a",b"ABCxxxHI").unwrap();
  fs.replace(b"/b",b"ABQxxxHJ").unwrap();
  let mut out=[EMPTY;2];assert_eq!(files(&fs,b"/a",b"/b",&mut out),Ok(2));
  assert_eq!((out[0].offset,out[1].offset),(2,7));
 }
 #[test]fn output_overflow_does_not_publish_partial_diff(){
  let mut out=[Range{offset:99,..EMPTY};1];
  assert_eq!(compare(b"abca",b"xbcy",&mut out),Err(Error::Full));
  assert_eq!(out[0].offset,99);
 }
}