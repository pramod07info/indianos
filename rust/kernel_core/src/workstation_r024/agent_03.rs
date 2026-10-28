//! R024/03: atomic batch file rename over actual project files.
//! All preimages and destination conflicts are checked before publishing.
use super::Error;
use crate::batch7_ramfs::{RamFs,PATH_MAX};
pub struct Rename<'a>{pub source:&'a[u8],pub target:&'a[u8],pub file_rev:u64}
pub fn rename<const F:usize,const IMAGE:usize>(fs:&mut RamFs<F>,
 expected:u64,changes:&[Rename<'_>])->Result<u64,Error>{
 if fs.version()!=expected{return Err(Error::Stale);}
 if changes.is_empty()||changes.len()>F{return Err(Error::Invalid);}
 for (i,ch) in changes.iter().enumerate(){
  if ch.source.len()<2||ch.target.len()<2||ch.source.len()>PATH_MAX||
   ch.target.len()>PATH_MAX||ch.source[0]!=b'/'||ch.target[0]!=b'/'||
   ch.source==ch.target||ch.source.windows(2).any(|w|w==b"..")||
   ch.target.windows(2).any(|w|w==b"..")||
   changes[..i].iter().any(|prev|prev.source==ch.source||prev.target==ch.target){
   return Err(Error::Invalid);
  }
  let (rev,_)=fs.metadata(ch.source).map_err(|_|Error::Missing)?;
  if rev!=ch.file_rev{return Err(Error::Stale);}
  if fs.metadata(ch.target).is_ok(){return Err(Error::Conflict);}
 }
 let mut original=[0u8;IMAGE];
 let len=fs.export_snapshot(&mut original).map_err(|_|Error::Full)?;
 let mut staged=RamFs::<F>::new();
 staged.import_snapshot(&original[..len]).map_err(|_|Error::Invalid)?;
 for ch in changes{
  staged.rename_if_version(ch.source,ch.target,ch.file_rev)
   .map_err(|_|Error::Conflict)?;
 }
 let mut final_img=[0u8;IMAGE];
 let n=staged.export_snapshot(&mut final_img).map_err(|_|Error::Full)?;
 fs.import_snapshot(&final_img[..n]).map_err(|_|Error::Invalid)?;
 Ok(fs.version())
}
pub fn preview<const F:usize>(fs:&RamFs<F>,changes:&[Rename<'_>])
 ->Result<usize,Error>{
 for (i,ch) in changes.iter().enumerate(){
  if changes[..i].iter().any(|x|x.source==ch.source||x.target==ch.target){
   return Err(Error::Conflict);
  }
  if fs.metadata(ch.source).is_err(){return Err(Error::Missing);}
  if fs.metadata(ch.target).is_ok(){return Err(Error::Conflict);}
 }Ok(changes.len())
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn batch_rename_updates_multiple_source_paths_without_losing_bytes(){
  let mut fs=RamFs::<2>::new();
  fs.replace(b"/a.rs",b"a").unwrap();fs.replace(b"/b.rs",b"b").unwrap();
  let changes=[Rename{source:b"/a.rs",target:b"/a.txt",file_rev:1},
   Rename{source:b"/b.rs",target:b"/b.txt",file_rev:2}];
  assert_eq!(preview(&fs,&changes),Ok(2));
  rename::<2,1024>(&mut fs,2,&changes).unwrap();
  assert!(fs.metadata(b"/a.txt").is_ok());assert!(fs.metadata(b"/b.txt").is_ok());
 }
 #[test]fn later_destination_collision_refuses_all_file_renames(){
  let mut fs=RamFs::<3>::new();
  fs.replace(b"/a",b"a").unwrap();fs.replace(b"/b",b"b").unwrap();
  fs.replace(b"/taken",b"x").unwrap();
  let changes=[Rename{source:b"/a",target:b"/new",file_rev:1},
   Rename{source:b"/b",target:b"/taken",file_rev:2}];
  assert_eq!(rename::<3,1024>(&mut fs,3,&changes),Err(Error::Conflict));
  assert!(fs.metadata(b"/a").is_ok());assert_eq!(fs.version(),3);
 }
 #[test]fn source_version_mismatch_rejects_operation_without_changes(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"a").unwrap();
  let changes=[Rename{source:b"/a",target:b"/z",file_rev:0}];
  assert_eq!(rename::<1,1024>(&mut fs,1,&changes),Err(Error::Stale));
 }
}
