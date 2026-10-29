//! R030/01: deterministic, bounded inventory of actual guest project files.
use super::Error;
use crate::batch7_ramfs::{RamFs,PATH_MAX};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Entry { pub path:[u8;PATH_MAX],pub len:usize,pub bytes:usize,pub revision:u64 }
pub const EMPTY:Entry=Entry{path:[0;PATH_MAX],len:0,bytes:0,revision:0};
pub fn inventory<const F:usize,const N:usize>(fs:&RamFs<F>,root:&[u8],
 out:&mut[Entry;N])->Result<usize,Error>{
 if root.len()<2||root[0]!=b'/'||root.ends_with(b"/"){return Err(Error::Invalid);}
 let mut stage=[EMPTY;N];let mut count=0;
 for i in 0..fs.count(){
  let mut path=[0;PATH_MAX];
  let (len,bytes,revision)=fs.entry(i,&mut path).map_err(|_|Error::Invalid)?
    .ok_or(Error::Missing)?;
  if !path[..len].starts_with(root)||path.get(root.len())!=Some(&b'/'){continue;}
  if count==N{return Err(Error::Full);}
  stage[count]=Entry{path,len,bytes,revision};count+=1;
 }
 stage[..count].sort_unstable_by(|a,b|a.path[..a.len].cmp(&b.path[..b.len]));
 *out=stage;Ok(count)
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn scans_real_project_with_stable_sort_and_revisions(){
  let mut fs=RamFs::<3>::new();
  fs.replace(b"/app/z",b"zz").unwrap();fs.replace(b"/app/a",b"x").unwrap();
  fs.replace(b"/apple/no",b"x").unwrap();
  let mut out=[EMPTY;2];assert_eq!(inventory(&fs,b"/app",&mut out),Ok(2));
  assert_eq!(&out[0].path[..out[0].len],b"/app/a");assert_eq!(out[0].revision,2);
 }
 #[test]fn short_inventory_does_not_publish_partial_results(){
  let mut fs=RamFs::<2>::new();fs.replace(b"/app/a",b"a").unwrap();
  fs.replace(b"/app/b",b"b").unwrap();let mut out=[EMPTY;1];
  assert_eq!(inventory(&fs,b"/app",&mut out),Err(Error::Full));assert_eq!(out,[EMPTY]);
 }
}