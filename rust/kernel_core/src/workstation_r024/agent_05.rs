//! R024/05: quota-aware real RamFs file writing and reclaimed-space accounting.
//! A standalone enforced API; direct RamFs writers must still be governed by kernel ingress.
use super::Error;
use crate::batch7_ramfs::{RamFs,PATH_MAX,CONTENT_MAX};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Quota{pub max_files:usize,pub max_bytes:usize,
 pub used_files:usize,pub used_bytes:usize}
pub fn inspect<const F:usize>(fs:&RamFs<F>,root:&[u8],
 max_files:usize,max_bytes:usize)->Result<Quota,Error>{
 if root.is_empty()||root[0]!=b'/'||root.len()>PATH_MAX{
  return Err(Error::Invalid);
 }
 let mut used_files=0usize;let mut used_bytes=0usize;
 for i in 0..fs.count(){
  let mut path=[0u8;PATH_MAX];
  let (n,size,_)=fs.entry(i,&mut path).map_err(|_|Error::Invalid)?
   .ok_or(Error::Missing)?;
  if path[..n].starts_with(root){
   used_files+=1;used_bytes=used_bytes.checked_add(size).ok_or(Error::Overflow)?;
  }
 }
 Ok(Quota{max_files,max_bytes,used_files,used_bytes})
}
pub fn put<const F:usize>(fs:&mut RamFs<F>,revision:u64,
 root:&[u8],path:&[u8],data:&[u8],
 file_limit:usize,byte_limit:usize)->Result<u64,Error>{
 if fs.version()!=revision{return Err(Error::Stale);}
 if root.is_empty()||root[0]!=b'/'||path.len()<root.len()||
  !path.starts_with(root)||data.len()>CONTENT_MAX{return Err(Error::Invalid);}
 let quota=inspect(fs,root,file_limit,byte_limit)?;
 let previous=fs.metadata(path).ok().map(|(_,size)|size);
 let next_count=quota.used_files+usize::from(previous.is_none());
 let next_bytes=quota.used_bytes.checked_sub(previous.unwrap_or(0))
  .and_then(|v|v.checked_add(data.len())).ok_or(Error::Overflow)?;
 if next_count>file_limit||next_bytes>byte_limit{return Err(Error::Full);}
 fs.replace(path,data).map_err(|_|Error::Full)
}
pub fn remaining(q:Quota)->(usize,usize){
 (q.max_files.saturating_sub(q.used_files),
  q.max_bytes.saturating_sub(q.used_bytes))
}
pub fn utilization(q:Quota)->Result<u8,Error>{
 if q.max_bytes==0{return Err(Error::Invalid);}
 Ok(((q.used_bytes.saturating_mul(100)/q.max_bytes).min(100)) as u8)
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn real_source_quota_tracks_create_overwrite_and_remaining_bytes(){
  let mut fs=RamFs::<2>::new();
  put(&mut fs,0,b"/p",b"/p/a",b"abcd",2,8).unwrap();
  put(&mut fs,1,b"/p",b"/p/a",b"abcdef",2,8).unwrap();
  let q=inspect(&fs,b"/p",2,8).unwrap();
  assert_eq!((q.used_files,q.used_bytes),(1,6));
  assert_eq!(remaining(q),(1,2));
 }
 #[test]fn quota_exhaustion_cannot_publish_partial_write(){
  let mut fs=RamFs::<1>::new();
  put(&mut fs,0,b"/p",b"/p/a",b"abcd",1,4).unwrap();
  assert_eq!(put(&mut fs,1,b"/p",b"/p/a",b"longer",1,4),Err(Error::Full));
  let mut out=[0;4];fs.read(b"/p/a",0,&mut out).unwrap();
  assert_eq!(&out,b"abcd");
 }
 #[test]fn wrong_project_root_and_stale_revision_are_denied(){
  let mut fs=RamFs::<1>::new();
  assert_eq!(put(&mut fs,0,b"/p",b"/q/a",b"x",1,10),Err(Error::Invalid));
  fs.replace(b"/p/a",b"a").unwrap();
  assert_eq!(put(&mut fs,0,b"/p",b"/p/a",b"b",1,10),Err(Error::Stale));
 }
}
