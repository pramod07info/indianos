//! R027/02: atomic rename transaction for several actual project files.
use super::Error;
use crate::batch7_ramfs::{RamFs,PATH_MAX};
pub struct Rename<'a>{pub from:&'a[u8],pub to:&'a[u8],pub revision:u64}
pub fn apply<const F:usize,const B:usize>(fs:&mut RamFs<F>,
 expected:u64,items:&[Rename<'_>])->Result<u64,Error>{
 if fs.version()!=expected{return Err(Error::Stale);}
 if items.is_empty()||items.len()>F{return Err(Error::Invalid);}
 for (i,r) in items.iter().enumerate(){
  if r.from.len()<2||r.to.len()<2||r.from.len()>PATH_MAX||
   r.to.len()>PATH_MAX||r.from[0]!=b'/'||r.to[0]!=b'/'||
   r.from==r.to||r.from.windows(2).any(|x|x==b"..")||
   r.to.windows(2).any(|x|x==b"..")||
   items[..i].iter().any(|prev|prev.to==r.to||prev.from==r.from){
   return Err(Error::Invalid);
  }
  if fs.metadata(r.from).map_err(|_|Error::Missing)?.0!=r.revision{
   return Err(Error::Stale);
  }
  if fs.metadata(r.to).is_ok(){return Err(Error::Conflict);}
 }
 let mut image=[0u8;B];let n=fs.export_snapshot(&mut image).map_err(|_|Error::Full)?;
 let mut staged=RamFs::<F>::new();
 staged.import_snapshot(&image[..n]).map_err(|_|Error::Invalid)?;
 for r in items{
  staged.rename_if_version(r.from,r.to,r.revision).map_err(|_|Error::Conflict)?;
 }
 let mut final_image=[0u8;B];
 let n=staged.export_snapshot(&mut final_image).map_err(|_|Error::Full)?;
 fs.import_snapshot(&final_image[..n]).map_err(|_|Error::Invalid)?;
 Ok(fs.version())
}
pub fn preflight<const F:usize>(fs:&RamFs<F>,items:&[Rename<'_>])
 ->Result<usize,Error>{
 for r in items{
  if fs.metadata(r.from).map_err(|_|Error::Missing)?.0!=r.revision{
   return Err(Error::Stale);
  }
  if fs.metadata(r.to).is_ok(){return Err(Error::Conflict);}
 }Ok(items.len())
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn rename_two_source_paths_as_a_single_file_tree_commit(){
  let mut fs=RamFs::<2>::new();
  fs.replace(b"/a.rs",b"a").unwrap();fs.replace(b"/b.rs",b"b").unwrap();
  let changes=[Rename{from:b"/a.rs",to:b"/a.txt",revision:1},
   Rename{from:b"/b.rs",to:b"/b.txt",revision:2}];
  apply::<2,2048>(&mut fs,2,&changes).unwrap();
  assert!(fs.metadata(b"/a.txt").is_ok());assert!(fs.metadata(b"/b.txt").is_ok());
 }
 #[test]fn later_name_collision_preserves_all_original_paths(){
  let mut fs=RamFs::<3>::new();
  fs.replace(b"/a",b"a").unwrap();fs.replace(b"/b",b"b").unwrap();
  fs.replace(b"/x",b"x").unwrap();
  let changes=[Rename{from:b"/a",to:b"/new",revision:1},
   Rename{from:b"/b",to:b"/x",revision:2}];
  assert_eq!(apply::<3,2048>(&mut fs,3,&changes),Err(Error::Conflict));
  assert!(fs.metadata(b"/a").is_ok());assert!(fs.metadata(b"/new").is_err());
 }
 #[test]fn stale_preimage_refuses_multi_rename(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"a").unwrap();
  let changes=[Rename{from:b"/a",to:b"/b",revision:0}];
  assert_eq!(apply::<1,1024>(&mut fs,1,&changes),Err(Error::Stale));
 }
}
