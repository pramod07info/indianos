//! R027/03: enforce project-specific real file and byte budgets at write ingress.
use super::Error;
use crate::batch7_ramfs::{RamFs,PATH_MAX,CONTENT_MAX};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Usage{pub files:usize,pub bytes:usize,pub cap_files:usize,pub cap_bytes:usize}
fn in_root(path:&[u8],root:&[u8])->bool{
 path.starts_with(root)&&path.get(root.len())==Some(&b'/')
}
pub fn measure<const F:usize>(fs:&RamFs<F>,root:&[u8],max_files:usize,
 max_bytes:usize)->Result<Usage,Error>{
 if root.len()<2||root[0]!=b'/'||root.len()>PATH_MAX{return Err(Error::Invalid);}
 let mut bytes=0usize;let mut files=0usize;
 for i in 0..fs.count(){
  let mut name=[0u8;PATH_MAX];
  let (n,size,_)=fs.entry(i,&mut name).map_err(|_|Error::Invalid)?
   .ok_or(Error::Missing)?;
  if in_root(&name[..n],root){
   files+=1;bytes=bytes.checked_add(size).ok_or(Error::Overflow)?;
  }
 }
 Ok(Usage{files,bytes,cap_files:max_files,cap_bytes:max_bytes})
}
pub fn write<const F:usize>(fs:&mut RamFs<F>,expected:u64,
 root:&[u8],path:&[u8],bytes:&[u8],max_files:usize,
 max_bytes:usize)->Result<u64,Error>{
 if fs.version()!=expected{return Err(Error::Stale);}
 if !in_root(path,root)||bytes.len()>CONTENT_MAX{return Err(Error::Invalid);}
 let u=measure(fs,root,max_files,max_bytes)?;
 let previous=fs.metadata(path).ok().map(|x|x.1);
 let next_files=u.files+usize::from(previous.is_none());
 let next_bytes=u.bytes.checked_sub(previous.unwrap_or(0))
  .and_then(|x|x.checked_add(bytes.len())).ok_or(Error::Overflow)?;
 if next_files>max_files||next_bytes>max_bytes{return Err(Error::Full);}
 fs.replace(path,bytes).map_err(|_|Error::Full)
}
pub fn remaining(u:Usage)->(usize,usize){
 (u.cap_files.saturating_sub(u.files),u.cap_bytes.saturating_sub(u.bytes))
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn project_quota_enforces_actual_source_file_storage(){
  let mut fs=RamFs::<2>::new();
  write(&mut fs,0,b"/p",b"/p/a",b"abc",1,4).unwrap();
  assert_eq!(write(&mut fs,1,b"/p",b"/p/b",b"z",1,4),Err(Error::Full));
  let u=measure(&fs,b"/p",1,4).unwrap();assert_eq!(remaining(u),(0,1));
 }
 #[test]fn quota_guard_prevents_partial_overwrite(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/p/a",b"keep").unwrap();
  assert_eq!(write(&mut fs,1,b"/p",b"/p/a",b"longer",1,4),Err(Error::Full));
  let mut out=[0;4];fs.read(b"/p/a",0,&mut out).unwrap();
  assert_eq!(&out,b"keep");
 }
 #[test]fn project_root_prefix_is_slash_bounded(){
  let mut fs=RamFs::<1>::new();
  assert_eq!(write(&mut fs,0,b"/p",b"/px/a",b"no",1,5),Err(Error::Invalid));
 }
}
