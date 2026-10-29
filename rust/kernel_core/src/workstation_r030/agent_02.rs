//! R030/02: fail-closed project quotas based on the current guest filesystem.
use super::Error;
use crate::batch7_ramfs::{RamFs,PATH_MAX};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Usage { pub files:usize,pub bytes:usize,pub largest:usize }
pub fn usage<const F:usize>(fs:&RamFs<F>,root:&[u8],
 max_files:usize,max_bytes:usize)->Result<Usage,Error>{
 if root.len()<2||root[0]!=b'/'||root.ends_with(b"/"){return Err(Error::Invalid);}
 let mut total=Usage{files:0,bytes:0,largest:0};
 for i in 0..fs.count(){
  let mut name=[0u8;PATH_MAX];
  let (len,size,_)=fs.entry(i,&mut name).map_err(|_|Error::Invalid)?
   .ok_or(Error::Missing)?;
  if !name[..len].starts_with(root)||name.get(root.len())!=Some(&b'/'){continue;}
  total.files=total.files.checked_add(1).ok_or(Error::Overflow)?;
  total.bytes=total.bytes.checked_add(size).ok_or(Error::Overflow)?;
  total.largest=total.largest.max(size);
  if total.files>max_files||total.bytes>max_bytes{return Err(Error::Full);}
 }
 Ok(total)
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn exact_project_quota_reports_actual_bytes(){
  let mut fs=RamFs::<3>::new();fs.replace(b"/p/a",b"abc").unwrap();
  fs.replace(b"/p/b",b"xy").unwrap();fs.replace(b"/private/a",b"secret").unwrap();
  assert_eq!(usage(&fs,b"/p",2,5),Ok(Usage{files:2,bytes:5,largest:3}));
 }
 #[test]fn quota_overflow_and_prefix_escape_rejected(){
  let mut fs=RamFs::<2>::new();fs.replace(b"/p/a",b"xyz").unwrap();
  fs.replace(b"/p2/a",b"123456").unwrap();
  assert_eq!(usage(&fs,b"/p",1,2),Err(Error::Full));
  assert_eq!(usage(&fs,b"/p",1,3).unwrap().files,1);
 }
}