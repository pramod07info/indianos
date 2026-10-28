//! R024/04: real project directory usage, extension breakdown and age of revisions.
//! Avoids synthetic folders: a directory is a validated prefix of RamFs absolute paths.
use super::Error;
use crate::batch7_ramfs::{RamFs,PATH_MAX};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Usage{pub files:usize,pub bytes:usize,pub latest_rev:u64,
 pub rust_files:usize,pub docs:usize,pub largest:usize}
pub fn under(path:&[u8],root:&[u8])->bool{
 path.starts_with(root)&&(root==b"/"||path.len()==root.len()||
 path.get(root.len())==Some(&b'/'))
}
pub fn stats<const F:usize>(fs:&RamFs<F>,root:&[u8])->Result<Usage,Error>{
 if root.is_empty()||root.len()>PATH_MAX||root[0]!=b'/'||
  root.windows(2).any(|w|w==b".."){return Err(Error::Invalid);}
 let mut u=Usage{files:0,bytes:0,latest_rev:0,rust_files:0,docs:0,largest:0};
 for i in 0..fs.count(){
  let mut name=[0u8;PATH_MAX];
  let (n,size,rev)=fs.entry(i,&mut name).map_err(|_|Error::Invalid)?
   .ok_or(Error::Missing)?;
  if !under(&name[..n],root){continue;}
  u.bytes=u.bytes.checked_add(size).ok_or(Error::Overflow)?;
  u.files+=1;u.latest_rev=u.latest_rev.max(rev);u.largest=u.largest.max(size);
  if name[..n].ends_with(b".rs"){u.rust_files+=1;}
  if name[..n].ends_with(b".md")||name[..n].ends_with(b".txt"){u.docs+=1;}
 }
 Ok(u)
}
pub fn content_ratio(files:usize,capacity:usize)->Result<u8,Error>{
 if capacity==0||files>capacity{return Err(Error::Invalid);}
 Ok(((files*100)/capacity) as u8)
}
pub fn free_slots<const F:usize>(fs:&RamFs<F>)->usize{F.saturating_sub(fs.count())}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn explorer_reports_actual_source_and_document_usage(){
  let mut fs=RamFs::<3>::new();
  fs.replace(b"/p/src/a.rs",b"abc").unwrap();
  fs.replace(b"/p/README.md",b"docs").unwrap();
  fs.replace(b"/other/x",b"ignored").unwrap();
  let u=stats(&fs,b"/p").unwrap();
  assert_eq!((u.files,u.bytes,u.rust_files,u.docs),(2,7,1,1));
  assert_eq!(free_slots(&fs),0);
 }
 #[test]fn path_boundary_prevents_other_project_usage_from_leaking(){
  let mut fs=RamFs::<2>::new();
  fs.replace(b"/src/a",b"a").unwrap();fs.replace(b"/srcX/secret",b"z").unwrap();
  assert_eq!(stats(&fs,b"/src").unwrap().files,1);
 }
 #[test]fn occupancy_percentage_and_root_validation_are_bounded(){
  assert_eq!(content_ratio(2,4),Ok(50));
  assert_eq!(content_ratio(5,4),Err(Error::Invalid));
  assert_eq!(stats(&RamFs::<1>::new(),b"relative"),Err(Error::Invalid));
 }
}
